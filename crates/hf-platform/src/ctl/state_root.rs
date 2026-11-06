//! Lexical state-root selection. Nothing is resolved through symlinks or created.
use hf_domain::ctl::state_root::{
    INVALID_PATH_MESSAGE, INVALID_ROOT_MESSAGE, InvalidStateRoot, StateRoot,
};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};
/// An override must be nonempty, absolute and free of NUL; otherwise the home default applies.
pub fn resolve_state_root(selected: Option<OsString>) -> Result<StateRoot, InvalidStateRoot> {
    match selected {
        Some(value) => {
            let text = value.to_str().ok_or(InvalidStateRoot {
                message: INVALID_ROOT_MESSAGE,
            })?;
            if text.is_empty() || text.contains('\0') || !Path::new(text).is_absolute() {
                return Err(InvalidStateRoot {
                    message: INVALID_ROOT_MESSAGE,
                });
            }
            normalize(Path::new(text), INVALID_ROOT_MESSAGE)
        }
        None => {
            let home = home_directory().ok_or(InvalidStateRoot {
                message: INVALID_PATH_MESSAGE,
            })?;
            normalize(&home.join(".harflow"), INVALID_PATH_MESSAGE)
        }
    }
}
/// Python's `Path.home()`: on Unix a set `HOME` wins even when empty (empty selects `/`)
/// and trailing separators are dropped; only an unset `HOME` consults the account database.
pub fn home_directory() -> Option<PathBuf> {
    #[cfg(unix)]
    if let Some(home) = std::env::var_os("HOME") {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let mut bytes = home.into_vec();
        while bytes.last() == Some(&b'/') {
            bytes.pop();
        }
        if bytes.is_empty() {
            bytes.push(b'/');
        }
        return Some(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)));
    }
    std::env::home_dir().filter(|path| !path.as_os_str().is_empty())
}
fn normalize(path: &Path, message: &'static str) -> Result<StateRoot, InvalidStateRoot> {
    let invalid = InvalidStateRoot { message };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| invalid.clone())?
            .join(path)
    };
    let mut output = PathBuf::new();
    let mut depth = 0usize;
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => output.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if depth > 0 {
                    output.pop();
                    depth -= 1;
                }
            }
            Component::Normal(part) => {
                output.push(part);
                depth += 1;
            }
        }
    }
    match output.to_str() {
        Some(text) if !text.is_empty() && !text.contains('\0') => {
            Ok(StateRoot::from_normalized(text.to_string()))
        }
        _ => Err(invalid),
    }
}
