//! Installed evidence from Start Menu links, App Paths and AppsFolder.
//! Packaged identities are Shell AUMIDs, verified on live processes by AppModel;
//! no identity is guessed from an executable name or an explorer command.
use crate::computer::{AppInstance, ApplicationTarget, Error};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};
use windows::{
    core::{w, Interface, PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, ERROR_NO_MORE_ITEMS, HANDLE,
        },
        Storage::Packaging::Appx::GetApplicationUserModelId,
        System::{
            Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ},
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Registry::*,
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
        UI::Shell::{
            ApplicationActivationManager, BHID_EnumItems, FOLDERID_AppsFolder,
            FOLDERID_CommonPrograms, FOLDERID_Programs, IApplicationActivationManager,
            IEnumShellItems, IShellItem, IShellItem2, IShellLinkW, PropertiesSystem::PROPERTYKEY,
            SHCreateItemInKnownFolder, SHGetKnownFolderPath, ShellLink, AO_NONE, KF_FLAG_DEFAULT,
            SIGDN_NORMALDISPLAY,
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    },
};

pub(super) struct Inventory {
    pub targets: Vec<ApplicationTarget>,
    pub gaps: Vec<String>,
}
fn failure(message: impl ToString) -> Error {
    Error::BackendFailed(message.to_string())
}
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}
fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())])
}
/// Convert only the documented filesystem canonical prefixes back to the
/// QueryFullProcessImageNameW namespace; never turn a UNC path into a relative ID.
fn image_identity(value: &str) -> Option<String> {
    let value = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
    };
    Path::new(&value)
        .is_absolute()
        .then(|| value.to_lowercase())
}
/// Same image-path namespace as QueryFullProcessImageNameW/process_stamp.
fn executable(path: &Path) -> Option<String> {
    if !path.is_absolute() || !path.extension()?.eq_ignore_ascii_case("exe") || !path.is_file() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    image_identity(canonical.to_str()?)
}
fn admit(targets: &mut BTreeMap<String, String>, path: &Path, name: String) {
    if let Some(id) = executable(path) {
        targets.entry(id).or_insert(name);
    }
}
unsafe fn shortcut(path: &Path) -> Result<Option<PathBuf>, Error> {
    let link: IShellLinkW =
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
    let persist: IPersistFile = link.cast().map_err(failure)?;
    let source = wide(path.as_os_str());
    persist
        .Load(PCWSTR(source.as_ptr()), STGM_READ)
        .map_err(failure)?;
    // Arguments may identify a different logical application (browser profiles,
    // uninstallers, packaged launchers). Never collapse these to the host exe.
    let mut args = vec![0u16; 32768];
    link.GetArguments(&mut args).map_err(failure)?;
    if !text(&args).trim().is_empty() {
        return Ok(None);
    }
    let mut target = vec![0u16; 32768];
    link.GetPath(&mut target, std::ptr::null_mut(), 0)
        .map_err(failure)?;
    let target = text(&target);
    Ok((!target.is_empty()).then(|| PathBuf::from(target)))
}
fn start_menu(
    targets: &mut BTreeMap<String, String>,
    gaps: &mut Vec<String>,
    check: &impl Fn() -> Result<(), Error>,
) -> Result<(), Error> {
    for folder in [FOLDERID_Programs, FOLDERID_CommonPrograms] {
        check()?;
        let raw = match unsafe { SHGetKnownFolderPath(&folder, KF_FLAG_DEFAULT, HANDLE::default()) }
        {
            Ok(raw) => raw,
            Err(_) => {
                gaps.push("start_menu_unavailable".into());
                continue;
            }
        };
        let folder = unsafe { raw.to_string() };
        unsafe {
            CoTaskMemFree(Some(raw.0 as _));
        }
        let Ok(folder) = folder else {
            gaps.push("start_menu_unreadable".into());
            continue;
        };
        let mut pending = vec![(PathBuf::from(folder), 0u32)];
        let mut visited = 0usize;
        while let Some((dir, depth)) = pending.pop() {
            check()?;
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(_) => {
                    gaps.push("start_menu_read_gap".into());
                    continue;
                }
            };
            for entry in entries {
                check()?;
                visited += 1;
                if visited > 16384 {
                    gaps.push("start_menu_entry_limit".into());
                    pending.clear();
                    break;
                }
                let Ok(entry) = entry else {
                    gaps.push("start_menu_read_gap".into());
                    continue;
                };
                let Ok(kind) = entry.file_type() else {
                    gaps.push("start_menu_read_gap".into());
                    continue;
                };
                // Junctions are also reparse points; do not traverse outside the
                // two authoritative Start Menu roots through a linked directory.
                use std::os::windows::fs::MetadataExt;
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        gaps.push("start_menu_read_gap".into());
                        continue;
                    }
                };
                if kind.is_symlink() || metadata.file_attributes() & 0x400 != 0 {
                    gaps.push("start_menu_reparse_point_skipped".into());
                    continue;
                }
                let path = entry.path();
                if kind.is_dir() {
                    if depth < 16 {
                        pending.push((path, depth + 1));
                    } else {
                        gaps.push("start_menu_depth_limit".into());
                    }
                } else if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
                {
                    match unsafe { shortcut(&path) } {
                        Ok(Some(target)) => {
                            let name = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            if executable(&target).is_some() {
                                admit(targets, &target, name);
                            } else {
                                gaps.push("shortcut_target_unsupported".into());
                            }
                        }
                        Ok(None) => {
                            gaps.push("shortcut_arguments_or_packaged_target_unsupported".into())
                        }
                        Err(_) => gaps.push("shortcut_read_gap".into()),
                    }
                }
            }
        }
    }
    Ok(())
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
fn app_paths(
    targets: &mut BTreeMap<String, String>,
    gaps: &mut Vec<String>,
    check: &impl Fn() -> Result<(), Error>,
) -> Result<(), Error> {
    // Read both registry views. HKCU evidence wins deterministically over HKLM.
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            check()?;
            let mut key = HKEY::default();
            let status = unsafe {
                RegOpenKeyExW(
                    root,
                    w!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
                    0,
                    KEY_READ | view,
                    &mut key,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                continue;
            }
            if status.is_err() {
                gaps.push("app_paths_read_gap".into());
                continue;
            }
            let key = Key(key);
            for index in 0..4096 {
                check()?;
                let mut name = vec![0u16; 512];
                let mut len = name.len() as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        key.0,
                        index,
                        PWSTR(name.as_mut_ptr()),
                        &mut len,
                        None,
                        PWSTR::null(),
                        None,
                        None,
                    )
                };
                if status == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if status.is_err() {
                    gaps.push("app_paths_read_gap".into());
                    break;
                }
                let mut child = HKEY::default();
                if unsafe {
                    RegOpenKeyExW(key.0, PCWSTR(name.as_ptr()), 0, KEY_READ | view, &mut child)
                }
                .is_err()
                {
                    gaps.push("app_paths_read_gap".into());
                    continue;
                }
                let child = Key(child);
                let mut buffer = vec![0u16; 32768];
                let mut bytes = (buffer.len() * 2) as u32;
                let mut kind = REG_VALUE_TYPE::default();
                let status = unsafe {
                    RegQueryValueExW(
                        child.0,
                        PCWSTR::null(),
                        None,
                        Some(&mut kind),
                        Some(buffer.as_mut_ptr() as *mut u8),
                        Some(&mut bytes),
                    )
                };
                if status.is_ok()
                    && kind == REG_SZ
                    && bytes as usize <= buffer.len() * 2
                    && bytes % 2 == 0
                {
                    let value = text(&buffer[..bytes as usize / 2]);
                    let path = PathBuf::from(value.trim_matches('"'));
                    let name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    if executable(&path).is_some() {
                        admit(targets, &path, name);
                    } else {
                        gaps.push("app_paths_target_unavailable".into());
                    }
                } else {
                    gaps.push("app_paths_value_unsupported".into());
                }
                if index == 4095 {
                    gaps.push("app_paths_entry_limit".into());
                }
            }
        }
    }
    Ok(())
}

// PKEY_AppUserModel_ID, documented Windows property-system key.
const APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};
unsafe fn shell_string(raw: PWSTR) -> Result<String, Error> {
    let value = raw.to_string().map_err(failure);
    CoTaskMemFree(Some(raw.0 as _));
    value
}
fn packaged_id(aumid: &str) -> Option<String> {
    let (family, application) = aumid.split_once('!')?;
    if family.is_empty()
        || application.is_empty()
        || application.contains('!')
        || aumid.len() > 4092
        || aumid
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return None;
    }
    Some(format!("win:{aumid}"))
}
fn apps_folder(
    targets: &mut BTreeMap<String, String>,
    gaps: &mut Vec<String>,
    check: &impl Fn() -> Result<(), Error>,
) -> Result<(), Error> {
    check()?;
    let folder: IShellItem = match unsafe {
        SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, PCWSTR::null())
    } {
        Ok(folder) => folder,
        Err(_) => {
            gaps.push("apps_folder_unavailable".into());
            return Ok(());
        }
    };
    let items: IEnumShellItems = match unsafe { folder.BindToHandler(None, &BHID_EnumItems) } {
        Ok(items) => items,
        Err(_) => {
            gaps.push("apps_folder_read_gap".into());
            return Ok(());
        }
    };
    for index in 0..16384 {
        check()?;
        let mut batch = [None];
        let mut fetched = 0;
        if unsafe { items.Next(&mut batch, Some(&mut fetched)) }.is_err() {
            gaps.push("apps_folder_read_gap".into());
            break;
        }
        if fetched == 0 {
            break;
        }
        let Some(item) = batch[0].take() else {
            gaps.push("apps_folder_read_gap".into());
            continue;
        };
        let details: IShellItem2 = match item.cast() {
            Ok(details) => details,
            Err(_) => {
                gaps.push("apps_folder_property_gap".into());
                continue;
            }
        };
        let aumid = match unsafe {
            details
                .GetString(&APP_USER_MODEL_ID)
                .map_err(failure)
                .and_then(|raw| shell_string(raw))
        } {
            Ok(aumid) => aumid,
            Err(_) => {
                gaps.push("apps_folder_property_gap".into());
                continue;
            }
        };
        // AppsFolder also contains classic applications; their non-packaged
        // AUMIDs are not a proof of process identity and use the Win32 sources.
        if let Some(app_id) = packaged_id(&aumid) {
            match unsafe {
                item.GetDisplayName(SIGDN_NORMALDISPLAY)
                    .map_err(failure)
                    .and_then(|raw| shell_string(raw))
            } {
                Ok(name) if !name.is_empty() => {
                    targets.insert(app_id, name);
                }
                _ => {
                    gaps.push("apps_folder_name_gap".into());
                }
            }
        }
        if index == 16383 {
            gaps.push("apps_folder_entry_limit".into());
        }
    }
    Ok(())
}
/// A process's actual AppModel identity, not its window host or image name.
pub(super) fn process_application_id(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 4096];
        let mut length = buffer.len() as u32;
        let result = GetApplicationUserModelId(process, &mut length, PWSTR(buffer.as_mut_ptr()));
        let _ = CloseHandle(process);
        if result.is_err() || length == 0 || length as usize > buffer.len() {
            return None;
        }
        packaged_id(&text(&buffer[..length as usize]))
    }
}
pub(super) fn installed(check: &impl Fn() -> Result<(), Error>) -> Result<Inventory, Error> {
    let mut targets = BTreeMap::new();
    let mut gaps = vec![];
    start_menu(&mut targets, &mut gaps, check)?;
    app_paths(&mut targets, &mut gaps, check)?;
    apps_folder(&mut targets, &mut gaps, check)?;
    let targets = targets
        .into_iter()
        .map(|(app_id, name)| ApplicationTarget {
            app_id,
            name,
            instances: vec![],
        })
        .collect();
    gaps.sort();
    gaps.dedup();
    Ok(Inventory { targets, gaps })
}
fn process_enumeration_done(error: &windows::core::Error) -> bool {
    // Toolhelp uses ERROR_NO_MORE_FILES, unlike RegEnumKeyExW's
    // ERROR_NO_MORE_ITEMS and the Shell enumerator's S_FALSE/fetched=0.
    error.code() == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0)
}
struct ProcessCandidate {
    app: AppInstance,
    parent_pid: u32,
}
fn application_members(
    candidates: Vec<ProcessCandidate>,
    window_owners: &HashSet<u32>,
) -> Vec<AppInstance> {
    let mut members: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            window_owners.contains(&candidate.app.pid)
                || !candidates.iter().any(|parent| {
                    parent.app.pid == candidate.parent_pid
                        && parent.app.identity == candidate.app.identity
                        && parent.app.generation < candidate.app.generation
                })
        })
        .map(|candidate| candidate.app.clone())
        .collect();
    members.sort_by_key(|app| (app.pid, app.generation));
    members
}
pub(super) fn running_instances(
    identity: &str,
    check: &impl Fn() -> Result<(), Error>,
) -> Result<Vec<AppInstance>, Error> {
    check()?;
    // Public request routing is opaque and retains caller spelling. Only native
    // image matching uses the normalized Win32 identity namespace.
    let canonical = if identity.starts_with("win:") {
        None
    } else {
        executable(Path::new(identity))
    };
    let identity = canonical.as_deref().unwrap_or(identity);
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(failure)?;
    let result = (|| {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
        let mut instances = vec![];
        loop {
            if let Err(error) = next {
                if process_enumeration_done(&error) {
                    break;
                }
                return Err(failure(error));
            }
            check()?;
            if let Ok(app) = super::native::process_stamp(entry.th32ProcessID) {
                let matches = if identity.starts_with("win:") {
                    process_application_id(app.pid).as_deref() == Some(identity)
                } else {
                    app.identity.0 == identity
                };
                // AUMID and creation stamp are separate native reads. Reject a
                // process that exited/reused its PID across that identity proof.
                if matches && super::native::process_stamp(app.pid).as_ref() == Ok(&app) {
                    instances.push(ProcessCandidate {
                        app,
                        parent_pid: entry.th32ParentProcessID,
                    });
                }
            }
            next = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        Ok(instances)
    })();
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let candidates = result?;
    // Reuse the native desktop/window proof, never a second raw EnumWindows
    // path. Window-owning children are real application instances even when
    // their same-image parent is still alive (e.g. a separate user window).
    let mut owners = HashSet::new();
    for window in super::native::all_windows()? {
        check()?;
        if super::native::user_application_window(window) {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(window, Some(&mut pid));
            }
            owners.insert(pid);
        }
    }
    Ok(application_members(candidates, &owners))
}
pub(super) fn resolve(
    app_id: &str,
    check: &impl Fn() -> Result<(), Error>,
) -> Result<ApplicationTarget, Error> {
    check()?;
    let direct = executable(Path::new(app_id));
    let identity = direct.as_deref().unwrap_or(app_id);
    let installed = installed(check)?
        .targets
        .into_iter()
        .find(|target| target.app_id == identity);
    let mut target = if let Some(target) = installed {
        target
    } else if let Some(identity) = direct {
        let name = Path::new(&identity)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        ApplicationTarget {
            app_id: identity,
            name,
            instances: vec![],
        }
    } else {
        return Err(Error::NativeFailure {
            code: "app_unavailable".into(),
            message: "No supported installed Win32 target or absolute executable path".into(),
        });
    };
    target.instances = running_instances(&target.app_id, check)?;
    // Server admission intentionally requires exact request/response routing.
    // Never replace that public key with our normalized executable identity.
    target.app_id = app_id.to_owned();
    Ok(target)
}
/// The caller checks its deadline immediately before this irreversible step,
/// then polls running_instances. Successful spawn is not proof of a GUI instance.
pub(super) fn launch_once(target: &ApplicationTarget) -> Result<(), Error> {
    if let Some(aumid) = target.app_id.strip_prefix("win:") {
        let manager: IApplicationActivationManager =
            unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER) }
                .map_err(failure)?;
        let aumid = wide(std::ffi::OsStr::new(aumid));
        unsafe { manager.ActivateApplication(PCWSTR(aumid.as_ptr()), PCWSTR::null(), AO_NONE) }
            .map_err(failure)?;
    } else {
        let executable =
            executable(Path::new(&target.app_id)).ok_or_else(|| Error::NativeFailure {
                code: "app_unavailable".into(),
                message: "Absolute executable target is no longer available".into(),
            })?;
        std::process::Command::new(executable)
            .spawn()
            .map_err(failure)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaged_identity_preserves_real_aumid_and_does_not_guess_classic_hosts() {
        let aumid = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
        assert_eq!(
            packaged_id(aumid).as_deref(),
            Some("win:Microsoft.WindowsCalculator_8wekyb3d8bbwe!App")
        );
        assert!(packaged_id("Calculator.exe").is_none());
        assert!(packaged_id("!App").is_none());
        assert!(packaged_id("Family!").is_none());
        assert!(packaged_id("Family!App!other").is_none());
        assert!(packaged_id("Family!../App").is_none());
        // The test binary is a classic executable, not a packaged application.
        assert!(process_application_id(std::process::id()).is_none());
    }
    #[test]
    fn canonical_path_codec_preserves_unc_without_network_access() {
        assert_eq!(
            image_identity(r"\\?\UNC\Server\Share\App.exe").as_deref(),
            Some(r"\\server\share\app.exe")
        );
        assert_eq!(
            image_identity(r"\\Server\Share\App.exe").as_deref(),
            Some(r"\\server\share\app.exe")
        );
        assert_eq!(
            image_identity(r"\\?\C:\Apps\App.exe").as_deref(),
            Some(r"c:\apps\app.exe")
        );
        assert!(image_identity(r"UNC\Server\Share\App.exe").is_none());
        assert!(image_identity(r"\\?\Volume{unknown}\App.exe").is_none());
    }
    #[test]
    fn identity_requires_existing_absolute_executable() {
        assert!(executable(Path::new("notepad.exe")).is_none());
        assert!(executable(Path::new(r"C:\does-not-exist-dtw.exe")).is_none());
        let exe = std::env::current_exe().unwrap();
        let id = executable(&exe).unwrap();
        assert!(!id.starts_with(r"\\?\"));
        assert_eq!(id, id.to_lowercase());
    }
    #[test]
    fn shell_link_reads_real_target_and_rejects_argument_identity() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            let path = std::env::temp_dir().join(format!("dtw-link-{}.lnk", uuid::Uuid::new_v4()));
            let result = (|| -> Result<(), Error> {
                let link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
                let exe = std::env::current_exe().map_err(failure)?;
                let target = wide(exe.as_os_str());
                link.SetPath(PCWSTR(target.as_ptr())).map_err(failure)?;
                let persist: IPersistFile = link.cast().map_err(failure)?;
                let file = wide(path.as_os_str());
                persist.Save(PCWSTR(file.as_ptr()), true).map_err(failure)?;
                assert_eq!(executable(&shortcut(&path)?.unwrap()), executable(&exe));
                link.SetArguments(w!("--different-application-profile"))
                    .map_err(failure)?;
                persist.Save(PCWSTR(file.as_ptr()), true).map_err(failure)?;
                assert!(shortcut(&path)?.is_none());
                Ok(())
            })();
            let _ = std::fs::remove_file(path);
            CoUninitialize();
            result.unwrap();
        }
    }
    #[test]
    fn resolve_preserves_request_route_and_matches_canonical_process_identity() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        }
        let result = (|| -> Result<(), Error> {
            let exe = std::env::current_exe().map_err(failure)?;
            let current = super::super::native::process_stamp(std::process::id())?;
            let canonical = executable(&exe).unwrap();
            for requested in [canonical.to_uppercase(), canonical.replace('\\', "/")] {
                let target = resolve(&requested, &|| Ok(()))?;
                // This is the exact admission guard in app_protocol/server.rs;
                // internal canonicalization must not cause wrong_app rejection.
                assert_eq!(target.app_id, requested);
                assert!(target.instances.contains(&current));
                assert!(running_instances(&requested, &|| Ok(()))?.contains(&current));
                assert_eq!(
                    executable(Path::new(&target.app_id)).as_deref(),
                    Some(canonical.as_str())
                );
            }
            Ok(())
        })();
        unsafe {
            CoUninitialize();
        }
        result.unwrap();
    }
    #[test]
    fn member_filter_keeps_window_owners_and_roots_but_not_same_image_helpers() {
        use crate::computer::AppIdentity;
        let candidate = |pid, parent_pid, birth, image: &str| ProcessCandidate {
            app: AppInstance {
                identity: AppIdentity(image.into()),
                pid,
                generation: birth,
            },
            parent_pid,
        };
        let candidates = vec![
            candidate(10, 1, 100, "browser.exe"),  // no-window root retained
            candidate(11, 10, 101, "browser.exe"), // helper
            candidate(12, 10, 102, "browser.exe"), // user window, same-image child retained
            candidate(13, 10, 99, "browser.exe"),  // PID 10 reused after child birth
            candidate(14, 10, 103, "other.exe"),   // parent has a different image
            candidate(15, 10, 100, "browser.exe"), // no proven earlier parent birth
        ];
        let owners = HashSet::from([12]);
        let result = application_members(candidates, &owners);
        assert_eq!(
            result.iter().map(|app| app.pid).collect::<Vec<_>>(),
            vec![10, 12, 13, 14, 15]
        );
    }
    #[test]
    fn toolhelp_termination_is_not_registry_termination() {
        let error =
            |code| windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(code));
        assert!(process_enumeration_done(&error(ERROR_NO_MORE_FILES.0)));
        assert!(!process_enumeration_done(&error(ERROR_NO_MORE_ITEMS.0)));
        assert!(!process_enumeration_done(&error(
            windows::Win32::Foundation::ERROR_ACCESS_DENIED.0
        )));
    }
    #[test]
    fn process_inventory_proves_current_incarnation() {
        let current = super::super::native::process_stamp(std::process::id()).unwrap();
        let instances = running_instances(&current.identity.0, &|| Ok(())).unwrap();
        assert!(instances.contains(&current));
    }
    #[test]
    fn cancellation_prevents_discovery_and_resolution() {
        assert!(matches!(
            installed(&|| Err(Error::Cancelled)),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            resolve("missing", &|| Err(Error::TimedOut)),
            Err(Error::TimedOut)
        ));
    }
}
