use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eframe::egui;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{ffi, sel};
use objc2_foundation::{NSArray, NSURL};

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// winit's app delegate lacks `application:openURLs:` and is out of reach before launch,
/// when the first file arrives, so the method goes on NSObject, which it inherits from.
pub fn listen() {
    extern "C-unwind" fn open_urls(
        _this: *mut AnyObject,
        _cmd: Sel,
        _app: *mut AnyObject,
        urls: *mut NSArray<NSURL>,
    ) {
        let Some(urls) = (unsafe { urls.as_ref() }) else {
            return;
        };
        let mut opened = OPENED.lock().unwrap();
        for url in urls.iter() {
            if let Some(path) = url.to_file_path() {
                opened.push(path);
            }
        }
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
    }
    let class = AnyClass::get(c"NSObject").expect("NSObject exists");
    let types: &CStr = c"v@:@@";
    let imp: Imp = unsafe {
        std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
            Imp,
        >(open_urls)
    };
    unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            sel!(application:openURLs:),
            imp,
            types.as_ptr(),
        );
    }
}

pub fn opened(ctx: &egui::Context) -> Vec<PathBuf> {
    let _ = CTX.set(ctx.clone());
    std::mem::take(&mut *OPENED.lock().unwrap())
}
