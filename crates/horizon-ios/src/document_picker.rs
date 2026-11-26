//! A single copied ZIP document, delivered back to the game on its main thread.
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id, mutability,
    rc::Retained,
    runtime::{NSObject, ProtocolObject},
};
use objc2_foundation::{MainThreadMarker, NSArray, NSObjectProtocol, NSString, NSURL};
use objc2_ui_kit::{UIApplication, UIDocumentPickerDelegate, UIDocumentPickerViewController};
use objc2_uniform_type_identifiers::UTType;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
};

declare_class!(
    struct PickerDelegate;
    unsafe impl ClassType for PickerDelegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HorizonModpackPickerDelegate";
    }
    impl DeclaredClass for PickerDelegate {}
    unsafe impl NSObjectProtocol for PickerDelegate {}
    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[method(documentPicker:didPickDocumentsAtURLs:)]
        fn picked(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            // Safety: the document picker returns copied, local file URLs. UIKit invokes
            // this delegate on the main thread; Foundation owns each URL during the call.
            if let Some(url) = urls.iter().next() {
                unsafe {
                    if url.isFileURL() && let Some(path) = url.path() {
                        PICKED.set(Some(PathBuf::from(path.to_string())));
                    }
                }
            }
            FINISHED.set(true);
        }
        #[method(documentPickerWasCancelled:)]
        fn cancelled(&self, _picker: &UIDocumentPickerViewController) { FINISHED.set(true); }
    }
);
thread_local! {
    // UIKit's delegate property is weak: retain both until the callback has returned.
    static CURRENT: RefCell<Option<(Retained<UIDocumentPickerViewController>, Retained<PickerDelegate>)>> = const { RefCell::new(None) };
    static PICKED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static FINISHED: Cell<bool> = const { Cell::new(false) };
}
pub(super) fn present() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if CURRENT.with_borrow(Option::is_some) {
        return;
    }
    // Safety: all UIKit work stays on the main thread. `public.zip-archive` is the system
    // ZIP UTType; asCopy gives the app its own temporary copy, without external-file access.
    unsafe {
        #[allow(deprecated)]
        let windows = UIApplication::sharedApplication(mtm).windows();
        let Some(window) = windows.iter().find(|w| w.isKeyWindow()) else {
            return;
        };
        let Some(root) = window.rootViewController() else {
            return;
        };
        if root.presentedViewController().is_some() {
            return;
        }
        let Some(zip_type) = UTType::typeWithIdentifier(&NSString::from_str("public.zip-archive"))
        else {
            return;
        };
        let types = NSArray::from_id_slice(&[zip_type]);
        let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
            mtm.alloc(),
            &types,
            true,
        );
        let delegate: Retained<PickerDelegate> = msg_send_id![mtm.alloc::<PickerDelegate>(), init];
        picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        picker.setAllowsMultipleSelection(false);
        FINISHED.set(false);
        CURRENT.set(Some((picker.clone(), delegate)));
        root.presentViewController_animated_completion(&picker, true, None);
    }
}
pub(super) fn take() -> Option<PathBuf> {
    MainThreadMarker::new()?;
    if FINISHED.replace(false) {
        CURRENT.set(None);
    }
    PICKED.take()
}
