//! The `virtx` command line: build, list, inspect, remove and pull images from a shell.
//!
//! [`run`] is the whole of it, and the `virtx` binary, the Python console script and the Node
//! `bin` each hand it their arguments. Each command runs on the console server
//! [`ensure_virtx`](crate::ensure_virtx) fetches, over [`ImageClient`](crate::image::ImageClient), and does what that
//! client's method does. The spelling follows `docker`'s, in both of its forms: `virtx image
//! <command>`, and the top-level `build`, `images`, `inspect`, `rmi` and `pull` most hands
//! know, which are the same commands.
//!
//! `command.rs` is the command tree and [`run`]; each command's arguments and handler are in
//! the file of its name, and `name.rs` is how an argument names an image: a ref in either of
//! its spellings, an image ID or a digest.

mod build;
mod command;
mod inspect;
mod ls;
mod name;
mod pull;
mod rm;

pub use command::run;
