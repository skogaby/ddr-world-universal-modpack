//! The user-owned file merges (design §4.6–§4.8). Pure: values in, values out;
//! no I/O, no logging — the install pipeline reads the files and logs the
//! reports.

pub mod csv;
pub mod csv_grammar;
pub mod json;
pub mod option_menu;
