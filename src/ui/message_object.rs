//! A `MessageSummary` wrapped as a GObject.
//!
//! `GtkListView` recycles row widgets against a `GListModel`, and a list model
//! can only hold GObjects — so the list is backed by these rather than by the
//! summaries directly. The wrapper carries the whole summary instead of
//! mirroring it into GObject properties: nothing binds to the fields, the rows
//! read them once at bind time.

use gtk::glib;
use gtk::subclass::prelude::*;

use crate::models::MessageSummary;

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct MessageObject {
        pub summary: RefCell<Option<MessageSummary>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageObject {
        const NAME: &'static str = "AirMailMessageObject";
        type Type = super::MessageObject;
    }

    impl ObjectImpl for MessageObject {}
}

glib::wrapper! {
    pub struct MessageObject(ObjectSubclass<imp::MessageObject>);
}

impl MessageObject {
    pub fn new(summary: MessageSummary) -> Self {
        let object: Self = glib::Object::new();
        object.imp().summary.replace(Some(summary));
        object
    }

    /// The summary this row stands for. Cloned rather than borrowed so a bind
    /// callback can hold it while it builds widgets.
    pub fn summary(&self) -> MessageSummary {
        self.imp()
            .summary
            .borrow()
            .clone()
            .expect("a message object always carries its summary")
    }

    pub fn id(&self) -> i64 {
        self.imp()
            .summary
            .borrow()
            .as_ref()
            .map(|s| s.id)
            .unwrap_or_default()
    }
}
