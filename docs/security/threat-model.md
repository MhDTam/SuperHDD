# Trust boundaries and outstanding work

The desktop process is unprivileged and its WebView accepts only bundled local
content. CSP disallows arbitrary remote script origins. Tauri capabilities permit
window controls; shell execution and arbitrary filesystem plugins are not exposed.
Discovery invokes one bundled constant PowerShell script; user input is never
interpolated into a command string. Disk choices are revalidated by physical ID.

The local service pipe grants SYSTEM/Administrators full access and interactive
users read/write access to a read-only command protocol. Remote clients are
rejected. FIRST_PIPE_INSTANCE prevents an unnoticed second server. The client
authenticates the peer PID against SCM. Mutations, command execution and privileged
download/file operations are rejected. RPC lifecycle and malformed-input tests
do not replace an independent IPC security audit.

The driver's device ACL permits SYSTEM and Administrators only. IOCTL access bits,
buffer sizes and protocol version are checked. The release configuration rejects
attachment. The experimental path now checks read/MDL bounds and restricts source
and backing to distinct, single-extent, file-backed VHD volumes. Only unmounted
raw sources are admitted; backing is newly created beneath a pinned non-reparse
NTFS parent, with exclusive access and no compression/encryption/sparse/reparse
flags. These guards are not runtime-validated. Virtual identities do not establish
separate physical host media or disposable data. Source encryption, mounted-volume
PnP integration, verifier and lifecycle qualification still require a lab. It must
never be exposed to an unprivileged broker as implemented.

PrimoCache is not bundled or modified. A discovered installation is not proof of
trusted vendor code, compatible driver status or permission to redistribute it.
Any executable used in a future privileged adapter must be installed in a trusted
ACL-protected location and signature-verified. Current inspection is unprivileged.

Downloads run as the caller. URL requests go only to the supplied HTTP/HTTPS
destination and its redirects. They must not be moved to a SYSTEM service without
an explicit credential/network/path isolation design. SHA-256 copy verification
does not establish publisher authenticity or malware safety. Destination NTFS
publication is no-overwrite; arbitrary existing files are not truncated or removed.
Interrupted publication, recovery and cross-process staging ownership now have
integration coverage, including killing a real worker during destination copy and
verifying the recovered SHA-256. General disk-full/removal fault injection and
long-duration concurrent quota stress still need coverage.

No Secure Boot, memory integrity, signature enforcement, firewall or antivirus
configuration is changed. The development installer never loads the kernel driver.
