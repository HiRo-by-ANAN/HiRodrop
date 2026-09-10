use std::ffi::{c_char, CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;

static SERVICE_SENDER: OnceLock<Sender<Vec<PathBuf>>> = OnceLock::new();

unsafe extern "C" {
    fn hirodrop_install_service_provider(callback: extern "C" fn(*const *const c_char, usize));
}

pub fn install() -> Receiver<Vec<PathBuf>> {
    let (sender, receiver) = mpsc::channel();
    SERVICE_SENDER
        .set(sender)
        .expect("Finder service provider must be installed only once");

    // SAFETY: App creation runs on the macOS main thread after NSApplication
    // exists. The callback has the exact C signature expected by the bridge.
    unsafe { hirodrop_install_service_provider(receive_files) };
    receiver
}

extern "C" fn receive_files(paths: *const *const c_char, count: usize) {
    if paths.is_null() || count == 0 || count > 10_000 {
        return;
    }

    // SAFETY: The Objective-C bridge keeps the pointer array and each
    // file-system representation alive for the duration of this callback.
    let paths = unsafe { std::slice::from_raw_parts(paths, count) };
    let files = paths
        .iter()
        .copied()
        .filter(|path| !path.is_null())
        .map(|path| {
            // SAFETY: NSURL's fileSystemRepresentation is NUL-terminated.
            let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
            PathBuf::from(OsStr::from_bytes(bytes))
        })
        .collect::<Vec<_>>();

    if !files.is_empty() {
        if let Some(sender) = SERVICE_SENDER.get() {
            let _ = sender.send(files);
        }
    }
}
