//! Row structs mirroring the tables in `db/migrations`.
//! IDs are UUID strings, timestamps are Unix milliseconds, JSON columns are raw strings.

mod attachment;
mod board;
mod card;
mod comment;
mod label;
mod list;
mod notification;
mod refresh_token;
mod time_entry;
mod user;
mod workspace;

pub use attachment::Attachment;
pub use board::Board;
pub use card::{Card, CardAssignee};
pub use comment::Comment;
pub use label::{CardLabel, Label};
pub use list::List;
pub use notification::Notification;
pub use refresh_token::RefreshToken;
pub use time_entry::TimeEntry;
pub use user::{User, UserRole};
pub use workspace::{Workspace, WorkspaceRole};
