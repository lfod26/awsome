use console::style;

pub struct Logger;

impl Logger {
    pub fn info(args: std::fmt::Arguments<'_>) {
        println!("{} {}", style("i").blue(), args);
    }

    pub fn warn(args: std::fmt::Arguments<'_>) {
        println!("{} {}", style("▲").yellow(), args);
    }

    // pub fn error(args: std::fmt::Arguments<'_>) {
    //     eprintln!("{} {}", style("✗").red(), args);
    // }

    pub fn success(args: std::fmt::Arguments<'_>) {
        println!("{} {}", style("✓").green(), args);
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
macro_rules! logger_error {
    ($($arg:tt)*) => {
        $crate::logger::Logger::error(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! logger_success {
    ($($arg:tt)*) => {
        $crate::logger::Logger::success(format_args!($($arg)*))
    };
}
