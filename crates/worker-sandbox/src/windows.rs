//! AppContainer token + explicit ACL grants + kill-on-close job. No network
//! capabilities are granted. A failed restriction is a failed launch.
use super::*;
use std::{
    ffi::{c_void, OsString},
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Isolation::*, *},
    Storage::FileSystem::*,
    System::{Console::*, JobObjects::*, Threading::*},
};
fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn check(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `Handle` only wraps a handle this module opened (job, process
            // or thread) and owns, so it is valid here and closed exactly once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
struct Container {
    sid: PSID,
    name: Vec<u16>,
    grants: Vec<PathBuf>,
}
impl Container {
    fn create() -> io::Result<Self> {
        let random = tempfile::Builder::new().prefix("identity-").tempfile()?;
        let name = wide(format!(
            "Overtalk.Worker.{}",
            random.path().file_name().unwrap().to_string_lossy()
        ));
        let mut sid = null_mut();
        // SAFETY: the name, display name and description are NUL-terminated UTF-16
        // buffers that live until the statement ends; there are no capabilities
        // (null, 0); `sid` receives a SID that Drop releases with FreeSid.
        let hr = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                wide("Isolated Overtalk worker").as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if hr < 0 {
            return Err(io::Error::other(format!(
                "AppContainer creation failed: {hr:#x}"
            )));
        }
        Ok(Self {
            sid,
            name,
            grants: vec![],
        })
    }
    fn grant(&mut self, path: &Path, write: bool) -> io::Result<()> {
        self.acl(
            path,
            if write {
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE
            } else {
                FILE_GENERIC_READ | FILE_GENERIC_EXECUTE
            },
            GRANT_ACCESS,
        )?;
        self.grants.push(path.to_owned());
        Ok(())
    }
    fn acl(&self, path: &Path, rights: u32, mode: ACCESS_MODE) -> io::Result<()> {
        let is_dir = path.is_dir();
        let path = wide(path);
        let mut old = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: `path` is a NUL-terminated UTF-16 buffer that outlives the call.
        // Only the DACL is requested: owner, group and SACL out-pointers are null,
        // and `old` points into `descriptor`, which is freed below with LocalFree.
        let result = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut old,
                null_mut(),
                &mut descriptor,
            )
        };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: rights,
            grfAccessMode: mode,
            grfInheritance: if is_dir {
                CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE
            } else {
                0
            },
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_UNKNOWN,
                ptstrName: self.sid.cast(),
            },
        };
        let mut new_acl = null_mut();
        // SAFETY: `entry` is one valid EXPLICIT_ACCESS_W whose trustee is this
        // container's live SID, `old` points into the still-allocated `descriptor`,
        // and `new_acl` receives a LocalAlloc'd ACL that is freed below.
        let result = unsafe { SetEntriesInAclW(1, &entry, old, &mut new_acl) };
        let result = if result == 0 {
            // SAFETY: `path` is NUL-terminated and outlives the call, and `new_acl`
            // is the valid ACL just built. Only the DACL is set; the other
            // descriptor parts are null.
            unsafe {
                SetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    new_acl,
                    null(),
                )
            }
        } else {
            result
        };
        // SAFETY: both pointers were allocated by the calls above with LocalAlloc,
        // are freed exactly once, and nothing uses them afterwards.
        unsafe {
            if !new_acl.is_null() {
                LocalFree(new_acl.cast());
            }
            if !descriptor.is_null() {
                LocalFree(descriptor.cast());
            }
        }
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        Ok(())
    }
}
impl Drop for Container {
    fn drop(&mut self) {
        for path in &self.grants {
            let _ = self.acl(path, 0, REVOKE_ACCESS);
        }
        // SAFETY: `name` is the NUL-terminated profile name created in `create`, and
        // `sid` is the SID CreateAppContainerProfile returned, released exactly once.
        unsafe {
            DeleteAppContainerProfile(self.name.as_ptr());
            FreeSid(self.sid);
        }
    }
}
fn quote(value: &OsStr) -> Vec<u16> {
    let mut out = vec![34];
    let mut slashes = 0;
    for c in value.encode_wide() {
        if c == 92 {
            slashes += 1;
            continue;
        }
        if c == 34 {
            out.extend(std::iter::repeat_n(92, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(92, slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(92, slashes * 2));
    out.push(34);
    out
}
fn low_integrity(path: &Path) -> io::Result<()> {
    // SAFETY: every string is a NUL-terminated UTF-16 temporary that outlives its
    // call. `acl` points into `descriptor`, which stays allocated until the
    // LocalFree after its last use; the other out-pointers are live locals.
    unsafe {
        let mut descriptor = null_mut();
        check(ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide("S:(ML;OICI;NW;;;LW)").as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ))?;
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = null_mut();
        let result = check(GetSecurityDescriptorSacl(
            descriptor,
            &mut present,
            &mut acl,
            &mut defaulted,
        ));
        let code = if result.is_ok() {
            SetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null(),
                acl,
            )
        } else {
            1
        };
        LocalFree(descriptor.cast());
        result?;
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(())
    }
}
/// The distinct, real handles among the standard ones, in order.
fn inheritable(handles: &[HANDLE]) -> Vec<HANDLE> {
    let mut list = Vec::with_capacity(handles.len());
    for &handle in handles {
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE && !list.contains(&handle) {
            list.push(handle);
        }
    }
    list
}
pub(super) fn run(policy: &Policy, program: &Path, mut args: Vec<OsString>) -> io::Result<i32> {
    let mut container = Container::create()?;
    for path in &policy.read {
        container.grant(path, false)?;
    }
    let mut outputs = Vec::new();
    for (index, path) in policy.write.iter().enumerate() {
        if path.starts_with(&policy.root) {
            low_integrity(path)?;
            container.grant(path, true)?;
        } else {
            // Never lower the integrity label of a user-owned output. Write in
            // the private sandbox and publish only after successful completion.
            let target = policy.root.join("work").join(format!(
                "output-{index}.{}",
                path.extension().and_then(OsStr::to_str).unwrap_or("data")
            ));
            for arg in &mut args {
                if arg == path.as_os_str() {
                    *arg = target.clone().into_os_string();
                }
            }
            outputs.push((target, path.clone()));
        }
    }
    let mut env_command = Command::new(program);
    super::safe_environment(&mut env_command, policy);
    let mut environment = Vec::new();
    let mut vars = env_command
        .get_envs()
        .filter_map(|(k, v)| v.map(|v| (k.to_owned(), v.to_owned())))
        .collect::<Vec<_>>();
    vars.sort_by_key(|(k, _)| k.to_string_lossy().to_ascii_uppercase());
    for (k, v) in vars {
        environment.extend(k.encode_wide());
        environment.push(61);
        environment.extend(v.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    let mut line = quote(program.as_os_str());
    for arg in &args {
        line.push(32);
        line.extend(quote(arg));
    }
    line.push(0);
    // SAFETY: the attribute list lives in `attrs`, sized by the first
    // InitializeProcThreadAttributeList call; `capabilities` and `inherited`, which
    // it points to, outlive `_guard`, which deletes it. `line` is a mutable
    // NUL-terminated command line and `environment` a double-NUL-terminated block
    // (safe_environment always sets PATH, LANG and HOME). Every handle passed on is
    // a standard handle or one this function created and owns through `Handle`.
    unsafe {
        let handles = [
            GetStdHandle(STD_INPUT_HANDLE),
            GetStdHandle(STD_OUTPUT_HANDLE),
            GetStdHandle(STD_ERROR_HANDLE),
        ];
        // Only the standard handles are inherited. A started-without-console
        // parent may have none, and stdout and stderr may share one; the
        // handle list rejects null and repeated entries.
        let inherited = inheritable(&handles);
        let count = if inherited.is_empty() { 1 } else { 2 };
        let mut bytes = 0;
        InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes);
        let mut attrs = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let attrs = attrs.as_mut_ptr().cast();
        check(InitializeProcThreadAttributeList(
            attrs, count, 0, &mut bytes,
        ))?;
        struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
        impl Drop for Attributes {
            fn drop(&mut self) {
                // SAFETY: the list was initialized successfully before this guard
                // was created, and it is deleted only here.
                unsafe { DeleteProcThreadAttributeList(self.0) }
            }
        }
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: container.sid,
            Capabilities: null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        // Declared after the values the list points to, so it is dropped first.
        let _guard = Attributes(attrs);
        check(UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (&capabilities as *const SECURITY_CAPABILITIES).cast(),
            size_of::<SECURITY_CAPABILITIES>(),
            null_mut(),
            null(),
        ))?;
        if !inherited.is_empty() {
            check(UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr().cast(),
                size_of_val(inherited.as_slice()),
                null_mut(),
                null(),
            ))?;
        }
        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attrs;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        let job = Handle(CreateJobObjectW(null(), null()));
        if job.0.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 128;
        check(SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ))?;
        let mut process: PROCESS_INFORMATION = zeroed();
        check(CreateProcessW(
            wide(program).as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            i32::from(!inherited.is_empty()),
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW,
            environment.as_ptr().cast::<c_void>(),
            wide(&policy.cwd).as_ptr(),
            &startup.StartupInfo,
            &mut process,
        ))?;
        let child = Handle(process.hProcess);
        let thread = Handle(process.hThread);
        if let Err(error) = check(AssignProcessToJobObject(job.0, child.0)) {
            TerminateProcess(child.0, 126);
            return Err(error);
        }
        if ResumeThread(thread.0) == u32::MAX {
            TerminateProcess(child.0, 126);
            return Err(io::Error::last_os_error());
        }
        WaitForSingleObject(child.0, INFINITE);
        let mut code = 126;
        check(GetExitCodeProcess(child.0, &mut code))?;
        // Reap descendants before reading their outputs. Otherwise a surviving
        // child could replace a staged file with a reparse point during publish.
        check(TerminateJobObject(job.0, code))?;
        if code == 0 {
            for (source, destination) in outputs {
                let metadata = fs::symlink_metadata(&source)?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || !source
                        .canonicalize()?
                        .starts_with(policy.root.canonicalize()?)
                {
                    return Err(io::Error::other("Refusing an indirect worker output"));
                }
                fs::copy(source, destination)?;
            }
        }
        Ok(code as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_handles_skip_missing_and_repeated_entries() {
        let (a, b) = (8usize as HANDLE, 12usize as HANDLE);
        assert_eq!(
            inheritable(&[null_mut(), a, INVALID_HANDLE_VALUE, a, b]),
            vec![a, b]
        );
        assert!(inheritable(&[null_mut(), INVALID_HANDLE_VALUE]).is_empty());
    }
}
