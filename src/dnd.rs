use crate::messages::{to_wide_null, WM_APP_DROP_PATHS};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use windows::core::{implement, BOOL, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, LPARAM, POINTL, S_OK,
    WPARAM,
};
use windows::Win32::System::Com::{
    IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, CF_HDROP, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{
    BHID_DataObject, DragQueryFileW, IShellItem, SHCreateItemFromParsingName, HDROP,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

#[implement(IDropTarget)]
pub struct PortalDropTarget {
    hwnd: HWND,
    is_drag_over: Cell<bool>,
}

impl PortalDropTarget {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            is_drag_over: Cell::new(false),
        }
    }

    fn has_hdrop(data_obj: Option<&IDataObject>) -> bool {
        let Some(obj) = data_obj else {
            return false;
        };
        let fmt = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe { obj.QueryGetData(&fmt).is_ok() }
    }

    fn extract_paths(data_obj: &IDataObject) -> Vec<PathBuf> {
        let fmt = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut paths = Vec::new();
        unsafe {
            let Ok(medium) = data_obj.GetData(&fmt) else {
                return paths;
            };
            let hdrop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None);
                if len > 0 {
                    let mut buf = vec![0u16; (len + 1) as usize];
                    let copied = DragQueryFileW(hdrop, i, Some(&mut buf));
                    if copied > 0 {
                        let s = String::from_utf16_lossy(&buf[..copied as usize]);
                        paths.push(PathBuf::from(s));
                    }
                }
            }
            release_stg_medium(medium);
        }
        paths
    }
}

unsafe fn release_stg_medium(mut medium: STGMEDIUM) {
    windows::Win32::System::Ole::ReleaseStgMedium(&mut medium);
}

impl IDropTarget_Impl for PortalDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: windows::core::Ref<'_, IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let valid = PortalDropTarget::has_hdrop(pdataobj.as_ref());
        self.is_drag_over.set(valid);
        unsafe {
            if !pdweffect.is_null() {
                *pdweffect = if valid {
                    choose_effect(grfkeystate)
                } else {
                    DROPEFFECT_NONE
                };
            }
            let _ = PostMessageW(
                Some(self.hwnd),
                WM_APP_DROP_PATHS,
                WPARAM(if valid { 1 } else { 0 }),
                LPARAM(0),
            );
        }
        Ok(())
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe {
            if !pdweffect.is_null() {
                *pdweffect = if self.is_drag_over.get() {
                    choose_effect(grfkeystate)
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.is_drag_over.set(false);
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_APP_DROP_PATHS, WPARAM(0), LPARAM(0));
        }
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows::core::Ref<'_, IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.is_drag_over.set(false);
        let paths = pdataobj
            .as_ref()
            .map(PortalDropTarget::extract_paths)
            .unwrap_or_default();

        let eff = choose_effect(grfkeystate);
        unsafe {
            if !pdweffect.is_null() {
                *pdweffect = if paths.is_empty() {
                    DROPEFFECT_NONE
                } else {
                    eff
                };
            }
            if !paths.is_empty() {
                let boxed = Box::new(paths);
                let ptr = Box::into_raw(boxed) as isize;
                let _ = PostMessageW(
                    Some(self.hwnd),
                    WM_APP_DROP_PATHS,
                    WPARAM(if eff == DROPEFFECT_MOVE { 3 } else { 2 }),
                    LPARAM(ptr),
                );
            } else {
                let _ = PostMessageW(Some(self.hwnd), WM_APP_DROP_PATHS, WPARAM(0), LPARAM(0));
            }
        }
        Ok(())
    }
}

fn choose_effect(mods: MODIFIERKEYS_FLAGS) -> DROPEFFECT {
    // Shift (0x04) -> Move, otherwise Copy
    if (mods.0 & 0x04) != 0 {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_COPY
    }
}

#[implement(IDropSource)]
struct PortalDropSource;

impl IDropSource_Impl for PortalDropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

pub fn start_file_drag(path: &Path) {
    let wide = to_wide_null(&path.to_string_lossy());
    unsafe {
        let Ok(item): windows::core::Result<IShellItem> =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
        else {
            return;
        };
        let Ok(data_obj): windows::core::Result<IDataObject> =
            item.BindToHandler(None, &BHID_DataObject)
        else {
            return;
        };
        let drop_source: IDropSource = PortalDropSource.into();
        let mut out_effect = DROPEFFECT_NONE;
        let _ = DoDragDrop(
            &data_obj,
            &drop_source,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            &mut out_effect,
        );
    }
}

pub fn copy_or_move_into_folder(paths: &[PathBuf], dest_dir: &Path, move_files: bool) {
    for src in paths {
        let Some(name) = src.file_name() else {
            continue;
        };
        let mut dst = dest_dir.join(name);
        if src == &dst {
            continue;
        }
        if dst.exists() {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Item".into());
            let ext = src
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            for n in 2..=99 {
                let candidate = dest_dir.join(format!("{stem} ({n}){ext}"));
                if !candidate.exists() {
                    dst = candidate;
                    break;
                }
            }
        }
        if move_files {
            let _ = std::fs::rename(src, &dst).or_else(|_| {
                copy_recursive(src, &dst)?;
                if src.is_dir() {
                    std::fs::remove_dir_all(src)
                } else {
                    std::fs::remove_file(src)
                }
            });
        } else {
            let _ = copy_recursive(src, &dst);
        }
    }
}

fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst)?;
        Ok(())
    }
}
