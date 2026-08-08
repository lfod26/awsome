use console::{StyledObject, style};

pub struct Logger;

impl Logger {
    pub fn info(args: std::fmt::Arguments<'_>) {
        println!("{} — {}", style("i").blue(), args);
    }

    pub fn warn(args: std::fmt::Arguments<'_>) {
        println!("{} — {}", style("▲").yellow(), args);
    }

    pub fn success(args: std::fmt::Arguments<'_>) {
        println!("{} — {}", style("✓").green(), args);
    }
}

#[macro_export]
macro_rules! logger_info {
    ($($arg:tt)*) => {
        $crate::logger::Logger::info(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! logger_warn {
    ($($arg:tt)*) => {
        $crate::logger::Logger::warn(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! logger_success {
    ($($arg:tt)*) => {
        $crate::logger::Logger::success(format_args!($($arg)*))
    };
}

pub fn dim_under(str: &str) -> StyledObject<&str> {
    style(str).dim().underlined()
}

pub fn bold(str: &str) -> StyledObject<&str> {
    style(str).bold()
}
