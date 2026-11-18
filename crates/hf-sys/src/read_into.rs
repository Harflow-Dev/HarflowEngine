//! Caller-buffer numeric MIB reads for macOS.

/// Reads one numeric MIB into caller-owned initialized storage.
///
/// Performs exactly one synchronous `sysctl` call after validation. No type
/// lookup, size query, allocation, retry, or value conversion is performed.
/// On success, returns the kernel-reported byte count, at most `output.len()`.
/// Callers must validate the expected record size and decode bytes themselves.
/// On error, output contents are unspecified and must not be used as a result.
/// A successful short or empty result is not evidence of a complete record.
///
/// `mib` must have 2..=12 components (the macOS CTL_MAXNAME bound); `output`
/// must be nonempty, preventing accidental size-query semantics. Invalid input
/// yields EINVAL, an impossible successful output length yields EOVERFLOW,
/// and syscall errors retain their raw errno, including ENOMEM and ESRCH.
/// Null newp and zero newlen request no value write. A numeric query may still
/// have kernel-defined read side effects; choose MIBs appropriate to the caller.
/// No hard timeout or cancellation of the synchronous kernel call is provided.
pub fn read_into(mib: &[libc::c_int], output: &mut [u8]) -> std::io::Result<usize> {
    // macOS public sys/sysctl.h: CTL_MAXNAME = 12.
    const MAX_COMPONENTS: usize = 12;
    if !(2..=MAX_COMPONENTS).contains(&mib.len()) || output.is_empty() {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    let namelen = libc::c_uint::try_from(mib.len())
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    // Darwin's C declaration takes a mutable pointer. Copy into exclusive,
    // bounded stack storage rather than casting away the shared input borrow.
    let mut name = [0; MAX_COMPONENTS];
    name[..mib.len()].copy_from_slice(mib);
    let capacity = output.len();
    let mut written = capacity;
    // SAFETY: name holds namelen initialized c_ints; output is exclusively
    // borrowed initialized writable storage of capacity bytes; written is a
    // valid size_t. sysctl respects oldlenp's input bound. No new value is
    // supplied. All storage remains live for the synchronous call. No typed
    // references are constructed from output and no input alias is mutated.
    #[expect(
        unsafe_code,
        reason = "the single reviewed sysctl call of this workspace"
    )]
    let result = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            namelen,
            output.as_mut_ptr().cast(),
            &mut written,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if written > capacity {
        return Err(std::io::Error::from_raw_os_error(libc::EOVERFLOW));
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::read_into;

    // macOS LP64 `struct kinfo_proc` (sys/sysctl.h): `extern_proc` followed by
    // `eproc`, 648 bytes in total; `extern_proc.p_pid` is an int at offset 40.
    const KINFO_PROC_SIZE: usize = 648;
    const P_PID_OFFSET: usize = 40;

    fn proc_mib(pid: libc::c_int) -> [libc::c_int; 4] {
        [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid]
    }

    fn decode_pid(record: &[u8]) -> libc::c_int {
        let bytes = record[P_PID_OFFSET..P_PID_OFFSET + 4].try_into().unwrap();
        libc::c_int::from_ne_bytes(bytes)
    }

    fn assert_errno(result: std::io::Result<usize>, errno: libc::c_int) {
        assert_eq!(result.unwrap_err().raw_os_error(), Some(errno));
    }

    #[test]
    fn rejects_component_counts_outside_two_to_twelve() {
        let mut output = [0u8; 8];
        for count in [0usize, 1, 13] {
            let mib = vec![libc::CTL_KERN; count];
            assert_errno(read_into(&mib, &mut output), libc::EINVAL);
        }
    }

    #[test]
    fn rejects_empty_output() {
        assert_errno(read_into(&proc_mib(1), &mut []), libc::EINVAL);
    }

    #[test]
    fn reads_the_current_process_record() {
        let pid = libc::c_int::try_from(std::process::id()).unwrap();
        let mut output = [0u8; KINFO_PROC_SIZE];
        assert_eq!(
            read_into(&proc_mib(pid), &mut output).unwrap(),
            KINFO_PROC_SIZE
        );
        assert_eq!(decode_pid(&output), pid);
    }

    #[test]
    fn reads_the_pid_one_record() {
        let mut output = [0u8; KINFO_PROC_SIZE];
        assert_eq!(
            read_into(&proc_mib(1), &mut output).unwrap(),
            KINFO_PROC_SIZE
        );
        assert_eq!(decode_pid(&output), 1);
    }

    #[test]
    fn reports_enomem_when_the_buffer_is_too_small() {
        let mut output = [0u8; 8];
        assert_errno(read_into(&proc_mib(1), &mut output), libc::ENOMEM);
    }

    #[test]
    fn reports_zero_bytes_for_a_nonexistent_pid() {
        let mut output = [0u8; KINFO_PROC_SIZE];
        assert_eq!(read_into(&proc_mib(99_999_999), &mut output).unwrap(), 0);
    }
}
