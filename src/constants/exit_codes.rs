//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//The numbers the program exits with, so scripts can tell what happened.

pub const SUCCESS: i32 = 0;
//Something went wrong while doing the work.
pub const FAILURE: i32 = 1;
//The command line itself was wrong. clap uses 2 for its own errors too.
pub const USAGE: i32 = 2;
//The VM, project or blueprint that was asked for does not exist.
pub const NOT_FOUND: i32 = 3;
//The person pressed Ctrl+C or Esc at a prompt. 130 is the shell convention.
pub const CANCELLED: i32 = 130;
