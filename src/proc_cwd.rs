//! Live working directory of a running shell process.
//!
//! amux runs the shell inside a ConPTY on Windows, and shells keep their *own*
//! process working directory in sync with `cd` (PowerShell's `Set-Location`,
//! cmd's `cd`, bash's `chdir` all end up in `SetCurrentDirectory`). Reading that
//! value back is what lets the activity-bar tooltip follow the terminal around
//! the filesystem without asking the shell to cooperate (OSC 7 shell
//! integration), which stock PowerShell and cmd do not send.
//!
//! The directory lives in the target process' PEB
//! (`PEB.ProcessParameters->CurrentDirectory.DosPath`), so the query is:
//! open the process, find the PEB, walk two pointers, read the string.

/// Read the current working directory of `pid`.
///
/// Returns `None` when the process is gone, access is denied, or the platform
/// has no implementation.
#[cfg(target_os = "windows")]
pub fn process_cwd(pid: u32) -> Option<String> {
    windows::process_cwd(pid)
}

#[cfg(not(target_os = "windows"))]
pub fn process_cwd(_pid: u32) -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;

    /// Opaque Win32 handle.
    type Handle = *mut c_void;

    /// `PROCESS_QUERY_LIMITED_INFORMATION`: enough to ask for the PEB.
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// `PROCESS_VM_READ`: needed by `ReadProcessMemory`.
    const PROCESS_VM_READ: u32 = 0x0010;
    /// `ProcessBasicInformation`, which answers the PEB address.
    const PROCESS_BASIC_INFORMATION: u32 = 0;

    /// `PEB.ProcessParameters` (x64 layout).
    const PEB_PROCESS_PARAMETERS: usize = 0x20;
    /// `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory` (x64 layout).
    const PROCESS_PARAMETERS_CURRENT_DIRECTORY: usize = 0x38;

    #[repr(C)]
    struct ProcessBasicInformationT {
        exit_status: i32,
        peb_base_address: *mut c_void,
        affinity_mask: usize,
        base_priority: usize,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }

    /// `UNICODE_STRING`: length in bytes plus a pointer to UTF-16 data.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }

    /// `CURDIR`: the directory string plus a handle to the same directory.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CurDir {
        dos_path: UnicodeString,
        handle: Handle,
    }

    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn CloseHandle(object: Handle) -> i32;
        fn ReadProcessMemory(
            process: Handle,
            base_address: *const c_void,
            buffer: *mut c_void,
            size: usize,
            bytes_read: *mut usize,
        ) -> i32;
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(
            process: Handle,
            info_class: u32,
            info: *mut c_void,
            info_len: u32,
            return_len: *mut u32,
        ) -> i32;
    }

    pub(super) fn process_cwd(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        // SAFETY: the handle is checked for null and closed before returning;
        // every read below validates its own return value.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if process.is_null() {
                return None;
            }
            let cwd = read_cwd(process);
            CloseHandle(process);
            cwd
        }
    }

    /// Walk PEB -> ProcessParameters -> CurrentDirectory and read the string.
    unsafe fn read_cwd(process: Handle) -> Option<String> {
        let mut info = ProcessBasicInformationT {
            exit_status: 0,
            peb_base_address: std::ptr::null_mut(),
            affinity_mask: 0,
            base_priority: 0,
            unique_process_id: 0,
            inherited_from_unique_process_id: 0,
        };
        let status = NtQueryInformationProcess(
            process,
            PROCESS_BASIC_INFORMATION,
            &mut info as *mut ProcessBasicInformationT as *mut c_void,
            std::mem::size_of::<ProcessBasicInformationT>() as u32,
            std::ptr::null_mut(),
        );
        if status < 0 || info.peb_base_address.is_null() {
            return None;
        }

        let parameters: usize =
            read_value(process, info.peb_base_address as usize + PEB_PROCESS_PARAMETERS)?;
        if parameters == 0 {
            return None;
        }

        let current_dir: CurDir =
            read_value(process, parameters + PROCESS_PARAMETERS_CURRENT_DIRECTORY)?;
        let byte_len = current_dir.dos_path.length as usize;
        if byte_len == 0 || current_dir.dos_path.buffer.is_null() {
            return None;
        }

        let mut utf16 = vec![0u16; byte_len / 2];
        let mut read = 0usize;
        let ok = ReadProcessMemory(
            process,
            current_dir.dos_path.buffer as *const c_void,
            utf16.as_mut_ptr() as *mut c_void,
            utf16.len() * 2,
            &mut read,
        );
        if ok == 0 {
            return None;
        }
        utf16.truncate(read / 2);

        let path = String::from_utf16_lossy(&utf16);
        let path = path.trim_matches('\0').to_string();
        (!path.is_empty()).then_some(path)
    }

    /// Read one `T` out of another process' address space.
    unsafe fn read_value<T: Copy>(process: Handle, address: usize) -> Option<T> {
        let mut value = std::mem::MaybeUninit::<T>::zeroed();
        let mut read = 0usize;
        let ok = ReadProcessMemory(
            process,
            address as *const c_void,
            value.as_mut_ptr() as *mut c_void,
            std::mem::size_of::<T>(),
            &mut read,
        );
        if ok == 0 || read != std::mem::size_of::<T>() {
            return None;
        }
        Some(value.assume_init())
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    /// The PEB walk is all raw pointers and hard-coded offsets; reading our own
    /// directory proves they line up with this build's process.
    #[test]
    fn reads_own_working_directory() {
        let expected = std::env::current_dir().expect("current dir");
        let found = super::process_cwd(std::process::id()).expect("own process cwd");

        // The PEB stores the directory as it was set; normalise both sides so
        // case and short/long name differences cannot fail the comparison.
        let found = std::fs::canonicalize(found).expect("canonical found");
        let expected = std::fs::canonicalize(expected).expect("canonical expected");
        assert_eq!(found, expected);
    }
}
