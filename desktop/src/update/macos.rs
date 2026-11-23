//! Sparkle's standard updater, loaded from `Contents/Frameworks` at launch. Loading it by path rather than linking it
//! keeps every build free of the framework: only the packaged bundle carries one.

use std::ptr;

use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSBundle, NSString};

/// The running `SPUStandardUpdaterController`. It schedules the daily check itself and lives as long as the app.
pub struct Updater(Retained<AnyObject>);

impl gpui::Global for Updater {}

impl Updater {
    /// Loads the framework beside the executable and starts the updater, or `None` outside a bundle that has one.
    pub fn start() -> Option<Self> {
        let framework = std::env::current_exe()
            .ok()?
            .parent()?
            .parent()?
            .join("Frameworks/Sparkle.framework");
        if !framework.is_dir() {
            return None;
        }
        let bundle = NSBundle::bundleWithPath(&NSString::from_str(&framework.to_string_lossy()));
        // SAFETY: the bundle is the Sparkle framework the packaging script signed into this app.
        let loaded = bundle.is_some_and(|bundle| unsafe { bundle.load() });
        let Some(class) = AnyClass::get(c"SPUStandardUpdaterController").filter(|_| loaded) else {
            eprintln!("{} did not load: no updates", framework.display());
            return None;
        };
        // SAFETY: `initWithStartingUpdater:updaterDelegate:userDriverDelegate:` takes a BOOL and two optional delegates,
        // and is called on the main thread, where the app starts.
        let controller: Retained<AnyObject> = unsafe {
            let allocated: Allocated<AnyObject> = msg_send![class, alloc];
            msg_send![
                allocated,
                initWithStartingUpdater: true,
                updaterDelegate: ptr::null::<AnyObject>(),
                userDriverDelegate: ptr::null::<AnyObject>()
            ]
        };
        Some(Self(controller))
    }

    /// Checks now and shows the result, as the menu item asks. A check already under way comes to the front.
    pub fn check(&self) {
        // SAFETY: `checkForUpdates:` is an action method that ignores its sender. Actions run on the main thread.
        unsafe {
            let _: () = msg_send![&*self.0, checkForUpdates: ptr::null::<AnyObject>()];
        }
    }
}
