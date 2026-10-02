//! Phone remote: control LocalFlow from the LocalFlow Remote phone app, through an
//! end-to-end encrypted relay. Off until the user turns it on and pairs a phone.

pub mod crypto;
pub mod handler;
pub mod link;
