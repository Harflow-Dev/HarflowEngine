use super::*;
impl Writer {
    pub(super) fn execute(&mut self, command: &StorageCommand) -> Result<(), String> {
        match command {
            StorageCommand::Bootstrap(_) => return Err("duplicate bootstrap".into()),
            StorageCommand::Claim => {
                self.session
                    .as_mut()
                    .ok_or("closed storage")?
                    .claim(self.until)
                    .map_err(error)?;
                self.refresh();
                self.gate(Stage::OwnerAfterClaim)?;
            }
            StorageCommand::RecordChild(child) => {
                self.session
                    .as_mut()
                    .ok_or("closed storage")?
                    .record_child(child, self.until)
                    .map_err(error)?;
                self.refresh();
                self.gate(Stage::OwnerAfterObserved)?;
            }
            StorageCommand::CreateRaw => {
                let raw = RawCapture::create(
                    Path::new(&self.root),
                    &self.snapshot.request.operation,
                    self.until,
                )
                .map_err(error)?;
                let (stdout, stderr) = raw.identities();
                self.raw_files = Some(RawIdentities { stdout, stderr });
                self.raw = Some(raw);
            }
            StorageCommand::Accept => self
                .session
                .as_mut()
                .ok_or("closed storage")?
                .accept(self.until)
                .map_err(error)?,
            StorageCommand::CaptureBegin(binding) => {
                let raw = self.raw.as_mut().ok_or("raw absent")?;
                raw.validate(0, 0, self.until).map_err(error)?;
                if raw.identities() != (binding.stdout.clone(), binding.stderr.clone()) {
                    return Err("capture raw identity mismatch".into());
                }
                self.session
                    .as_mut()
                    .ok_or("closed storage")?
                    .begin_capture(binding, self.until)
                    .map_err(error)?;
            }
            StorageCommand::Append {
                stream,
                bytes,
                drain,
            } => self.append(*stream, bytes, drain.as_ref())?,
            StorageCommand::KnownLoss => {
                let mut next = self
                    .capture
                    .as_ref()
                    .ok_or("capture absent")?
                    .progress
                    .clone();
                next.revision += 1;
                next.loss = true;
                self.session
                    .as_mut()
                    .ok_or("closed storage")?
                    .publish_capture(&next, self.until)
                    .map_err(error)?;
            }
            StorageCommand::Finish(exit) => self.finish(*exit)?,
            StorageCommand::Close => {
                self.refresh();
                drop(self.raw.take());
                drop(self.session.take());
            }
        }
        self.refresh();
        self.gate(Stage::OwnerBeforeAck)
    }
    fn finish(&mut self, exit: hf_domain::process::ProcessExit) -> Result<(), String> {
        self.gate(Stage::CaptureBeforeTerminal)?;
        let p = &self.capture.as_ref().ok_or("capture absent")?.progress;
        self.raw
            .as_mut()
            .ok_or("raw absent")?
            .validate(p.stdout, p.stderr, self.until)
            .map_err(error)?;
        self.session
            .as_mut()
            .ok_or("closed storage")?
            .finish_capture(exit, self.until)
            .map_err(error)?;
        self.refresh();
        self.gate(Stage::CaptureAfterTerminal)?;
        Ok(())
    }
    fn append(
        &mut self,
        stream: Stream,
        bytes: &[u8],
        drain: Option<&TrustedDrainEvidence>,
    ) -> Result<(), String> {
        let mut next = self
            .capture
            .as_ref()
            .ok_or("capture absent")?
            .progress
            .clone();
        let mut raw = self.raw.take().ok_or("raw absent")?;
        let (stdout, stderr) = match stream {
            Stream::Stdout => (bytes, &[][..]),
            Stream::Stderr => (&[][..], bytes),
        };
        let result = raw.append(
            next.stdout,
            next.stderr,
            stdout,
            stderr,
            self.until,
            |stage| self.gate(stage).map_err(std::io::Error::other),
        );
        self.raw = Some(raw);
        let offsets = result.map_err(error)?;
        next.stdout = offsets.stdout;
        next.stderr = offsets.stderr;
        next.revision += 1;
        if let Some(d) = drain {
            // Coordinator checked joined reader, input and accepted-data reconciliation before I/O.
            next.stdout_eof = d.stdout_eof;
            next.stderr_eof = d.stderr_eof;
        }
        self.session
            .as_mut()
            .ok_or("closed storage")?
            .publish_capture(&next, self.until)
            .map_err(error)?;
        self.refresh();
        self.gate(Stage::CaptureAfterPublish)
    }
}
