//! Administrator/System-only native storage for service configuration and leases.
//! No inherited user ACEs, reparse points, EFS/compression or hard links accepted.
use super::*;
use windows_sys::Win32::Security::{Authorization::*, *};
const SDDL: &str = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn descriptor(sddl: &str) -> Result<Descriptor> {
    let mut raw = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(sddl)?.as_ptr(),
            1,
            &mut raw,
            null_mut(),
        )
    } == 0
    {
        return Err(error("Create private service security", unsafe {
            GetLastError()
        }));
    }
    Ok(Descriptor(raw))
}
fn trusted(sid: PSID) -> bool {
    unsafe {
        !sid.is_null()
            && (IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0)
    }
}
fn validate_descriptor(raw: PSECURITY_DESCRIPTOR, protected: bool) -> Result<()> {
    let mut owner = null_mut();
    let mut defaulted = 0;
    let mut control = 0;
    let mut revision = 0;
    let mut present = 0;
    let mut dacl = null_mut();
    if unsafe { GetSecurityDescriptorOwner(raw, &mut owner, &mut defaulted) } == 0
        || !trusted(owner)
        || unsafe { GetSecurityDescriptorControl(raw, &mut control, &mut revision) } == 0
        || (protected && control & SE_DACL_PROTECTED == 0)
        || unsafe { GetSecurityDescriptorDacl(raw, &mut present, &mut dacl, &mut defaulted) } == 0
        || present == 0
        || dacl.is_null()
        || unsafe { (*dacl).AceCount } != 2
    {
        return Err(
            "Service storage owner/DACL is not private System/Administrators security.".into(),
        );
    }
    let mut system = false;
    let mut administrators = false;
    for index in 0..2 {
        let mut ace = null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err("Cannot inspect service storage ACE.".into());
        }
        let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
        if unsafe { (*ace).Header.AceType } != 0
            || unsafe { (*ace).Header.AceSize } < 12
            || unsafe { (*ace).Mask } != FILE_ALL_ACCESS
            || unsafe { (*ace).Header.AceFlags } & INHERIT_ONLY_ACE as u8 != 0
        {
            return Err("Unexpected access rule in private service storage.".into());
        }
        let sid = unsafe { std::ptr::addr_of_mut!((*ace).SidStart).cast() };
        if unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } != 0 {
            system = true;
        } else if unsafe { IsWellKnownSid(sid, WinBuiltinAdministratorsSid) } != 0 {
            administrators = true;
        } else {
            return Err("Private service storage grants access to another principal.".into());
        }
    }
    if !system || !administrators {
        return Err("Private storage is missing its service/administrator owner.".into());
    }
    Ok(())
}
fn verify(file: &File, directory: bool) -> Result<()> {
    let attributes = file
        .metadata()
        .map_err(|e| e.to_string())?
        .file_attributes();
    if attributes
        & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_ENCRYPTED)
        != 0
        || (attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
    {
        return Err("Unexpected service storage file type or attributes.".into());
    }
    let mut raw = null_mut();
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut raw,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(error("Inspect private service storage security", code));
    }
    let d = Descriptor(raw);
    validate_descriptor(d.0, directory)?;
    if !directory {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.nNumberOfLinks != 1
        {
            return Err("Service state cannot have an alternate hard link.".into());
        }
    }
    Ok(())
}
pub struct ProtectedDir {
    path: PathBuf,
    _pins: Vec<File>,
}
impl ProtectedDir {
    pub fn create(path: &Path) -> Result<Self> {
        let d = descriptor(SDDL)?;
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: d.0,
            bInheritHandle: 0,
        };
        let p = wide(path.to_str().ok_or("Invalid service storage path")?)?;
        if unsafe { CreateDirectoryW(p.as_ptr(), &security) } == 0
            && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
        {
            return Err(error("Create protected service directory", unsafe {
                GetLastError()
            }));
        }
        Self::open(path)
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err("Private service storage needs an absolute path.".into());
        }
        let mut prefix = PathBuf::new();
        let mut pins = Vec::new();
        for component in path.components() {
            prefix.push(component.as_os_str());
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            let file = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&prefix)
                .map_err(|e| e.to_string())?;
            let flags = file
                .metadata()
                .map_err(|e| e.to_string())?
                .file_attributes();
            if flags
                & (FILE_ATTRIBUTE_REPARSE_POINT
                    | FILE_ATTRIBUTE_COMPRESSED
                    | FILE_ATTRIBUTE_ENCRYPTED)
                != 0
            {
                return Err(
                    "Private storage ancestry contains a reparse, compressed or EFS directory."
                        .into(),
                );
            }
            pins.push(file);
        }
        verify(pins.last().ok_or("Missing service directory")?, true)?;
        Ok(Self {
            path: path.to_path_buf(),
            _pins: pins,
        })
    }
    pub fn file(&self, name: &str, create_new: bool) -> Result<File> {
        self.open_file(name,create_new,false,false,false)
    }
    /// Private cache payload; exclusively held and opened without the Windows
    /// file-data cache. Ownership/ACL/reparse/hard-link checks remain mandatory.
    pub fn payload(&self,name:&str,create_new:bool)->Result<File>{
        self.open_file(name,create_new,true,false,false)
    }
    /// Immutable configuration pin shared by service and worker.
    pub fn read_file(&self,name:&str)->Result<File>{self.open_file(name,false,false,true,false)}
    /// Read a journal concurrently with its trusted writer; all file identities
    /// and ACL checks still apply. The returned handle cannot modify the file.
    pub fn read_shared_file(&self,name:&str)->Result<File>{self.open_file(name,false,false,true,true)}
    fn open_file(&self,name:&str,create_new:bool,payload:bool,read_only:bool,shared:bool)->Result<File>{
        if name.is_empty()
            || name == ".."
            || name.starts_with('.')
            || name.ends_with('.')
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        {
            return Err("Invalid private state filename.".into());
        }
        let p = wide(
            self.path
                .join(name)
                .to_str()
                .ok_or("Invalid private state path")?,
        )?;
        let d = descriptor("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)")?;
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: d.0,
            bInheritHandle: 0,
        };
        let raw = unsafe {
            CreateFileW(
                p.as_ptr(),
                if read_only {GENERIC_READ}else if payload {GENERIC_READ|GENERIC_WRITE|DELETE}else{FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE},
                if payload {0}else if shared {FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE}else{FILE_SHARE_READ},
                &security,
                if create_new {
                    CREATE_NEW
                } else {
                    OPEN_EXISTING
                },
                FILE_FLAG_OPEN_REPARSE_POINT | if payload {FILE_FLAG_NO_BUFFERING|FILE_FLAG_WRITE_THROUGH}else{0},
                null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(error("Open protected service state", unsafe {
                GetLastError()
            }));
        }
        let file = unsafe { File::from_raw_handle(raw) };
        verify(&file, false)?;
        Ok(file)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
pub fn remove_file(file: File) -> Result<()> {
    verify(&file, false)?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(error("Remove own protected service state", unsafe {
            GetLastError()
        }));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_acl_requires_both_trusted_principals_and_blocks_inherited_or_user_rules() {
        let d = descriptor(SDDL).unwrap();
        validate_descriptor(d.0, true).unwrap();
        for text in [
            "O:BAG:BAD:P(A;;FA;;;WD)(A;;FA;;;BA)",
            "O:BAG:BAD:(A;;FA;;;SY)(A;;FA;;;BA)",
            "O:WDG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)",
            "O:BAG:BAD:P(A;;FR;;;SY)(A;;FA;;;BA)",
        ] {
            let d = descriptor(text).unwrap();
            assert!(validate_descriptor(d.0, true).is_err(), "{text}");
        }
    }
}
