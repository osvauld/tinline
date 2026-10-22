//! `sink_adapter!`: hands a runtime's `*Sink` callbacks to the app's UniFFI `*Events` object.
//!
//! The runtimes carry no UniFFI, so each defines a plain `*Sink` trait; the app implements the
//! matching `*Events` trait. The orphan rule rules out a blanket impl, so each pair lists the
//! callbacks once here: `sink_adapter!(pub(crate) ChatEventsAdapter(ChatEvents) => ChatSink { fn
//! on_chat_changed(chat: Chat); .. })`.

macro_rules! sink_adapter {
    (
        $(#[$meta:meta])*
        $vis:vis $adapter:ident($events:path) => $sink:path {
            $(fn $callback:ident($($arg:ident: $ty:ty),*);)*
        }
    ) => {
        $(#[$meta])*
        $vis struct $adapter(pub(crate) ::std::sync::Arc<dyn $events>);

        impl $sink for $adapter {
            $(fn $callback(&self, $($arg: $ty),*) {
                self.0.$callback($($arg),*);
            })*
        }
    };
}
