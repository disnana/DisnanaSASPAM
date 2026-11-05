//! Ubuntu PAM account hook: runs after the SSH authentication step.
//! Only /etc/pam.d/sshd is modified. Never put this in common-account.
use libc::{c_char, c_int, c_void};
use saspam_protocol::{PamCommand, Reply, Status, PAM_SOCKET, VERSION};
use std::{ffi::CStr, io::{BufRead, BufReader, Read, Write}, os::unix::net::UnixStream, panic::AssertUnwindSafe, time::Duration};

#[repr(C)]
pub struct PamHandle { _private: [u8; 0] }
#[link(name = "pam")]
extern "C" {
    fn pam_get_item(pamh: *mut PamHandle, item_type: c_int, item: *mut *const c_void) -> c_int;
}
const PAM_SUCCESS: c_int = 0;
const PAM_PERM_DENIED: c_int = 6;
const PAM_IGNORE: c_int = 25;
const PAM_SERVICE: c_int = 1;
const PAM_USER: c_int = 2;
const PAM_RHOST: c_int = 4;

unsafe fn pam_item(pamh: *mut PamHandle, typ: c_int) -> Option<String> {
    let mut raw: *const c_void = std::ptr::null();
    if pam_get_item(pamh, typ, &mut raw) != PAM_SUCCESS || raw.is_null() { return None; }
    Some(CStr::from_ptr(raw.cast::<c_char>()).to_string_lossy().into_owned())
}
fn check(pamh: *mut PamHandle) -> c_int {
    if pamh.is_null() { return PAM_PERM_DENIED; }
    // PAM is a native FFI boundary; the handle/item pointers are owned by libpam.
    let service = unsafe { pam_item(pamh, PAM_SERVICE) }.unwrap_or_default();
    // Defense in depth: even if loaded into console/sudo stack accidentally, do not gate them.
    if service != "sshd" { return PAM_IGNORE; }
    let Some(user) = (unsafe { pam_item(pamh, PAM_USER) }) else { return PAM_PERM_DENIED; };
    if user.is_empty() || user.len() > 128 { return PAM_PERM_DENIED; }
    let remote_host = unsafe { pam_item(pamh, PAM_RHOST) }.unwrap_or_else(|| "unknown".into());
    if remote_host.len() > 255 { return PAM_PERM_DENIED; }
    let request = PamCommand::Authorize { version: VERSION, user, service, remote_host };
    let Ok(mut stream) = UnixStream::connect(PAM_SOCKET) else { return PAM_PERM_DENIED; };
    if stream.set_read_timeout(Some(Duration::from_secs(195))).is_err() ||
       stream.set_write_timeout(Some(Duration::from_secs(3))).is_err() { return PAM_PERM_DENIED; }
    let Ok(mut line) = serde_json::to_vec(&request) else { return PAM_PERM_DENIED; };
    line.push(b'\n');
    if stream.write_all(&line).is_err() { return PAM_PERM_DENIED; }
    let mut reply_line = String::new();
    if BufReader::new(stream).take(4096).read_line(&mut reply_line).is_err() { return PAM_PERM_DENIED; }
    let Ok(reply) = serde_json::from_str::<Reply>(&reply_line) else { return PAM_PERM_DENIED; };
    if reply.ok && reply.status == Some(Status::Approved) { PAM_SUCCESS } else { PAM_PERM_DENIED }
}

/// Linux-PAM account management hook. OpenSSH invokes this after successful
/// public-key/FIDO/password authentication when UsePAM=yes.
#[no_mangle]
pub extern "C" fn pam_sm_acct_mgmt(pamh: *mut PamHandle, _flags: c_int, _argc: c_int, _argv: *const *const c_char) -> c_int {
    std::panic::catch_unwind(AssertUnwindSafe(|| check(pamh))).unwrap_or(PAM_PERM_DENIED)
}
