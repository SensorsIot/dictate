//! Notification Center banners, attributed to the dictate bundle.

use std::sync::Once;
use std::thread;

pub const BUNDLE_ID: &str = "ch.sensorsiot.dictate";

static ATTRIBUTE: Once = Once::new();

/// Returns at once: delivering a banner can wait on the notification service.
pub fn show(title: &str, detail: &str) {
    let (title, detail) = (title.to_owned(), detail.to_owned());
    thread::spawn(move || {
        ATTRIBUTE.call_once(|| {
            let _ = mac_notification_sys::set_application(BUNDLE_ID);
        });
        let _ = mac_notification_sys::send_notification(&title, None, &detail, None);
    });
}
