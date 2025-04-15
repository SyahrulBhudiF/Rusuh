use std::fs::File;
use std::io::{self, Read};

fn main() {
    // Errors in Rust are categorized into two types:
    // 1. Unrecoverable errors (handled with `panic!`)
    // 2. Recoverable errors (handled with `Result<T, E>`)

    // ========================
    // UNRECOVERABLE ERRORS
    // ========================

    // These are critical problems that stop the program immediately.
    // Example: Indexing out of bounds will cause panic.
    // let numbers = [1, 2, 3];
    // println!("Number at index 10: {}", numbers[10]); // This will panic

    // You can also cause a panic manually using the panic! macro
    // panic!("Something went wrong!"); // Uncomment to try it

    // ========================
    // RECOVERABLE ERRORS
    // ========================

    // Recoverable errors are expected to happen and should be handled properly.
    // These use the Result<T, E> enum, which has two variants:
    // - Ok(T): The operation succeeded
    // - Err(E): The operation failed

    // Example: Opening a file that may not exist
    let file_result = File::open("hello.txt");

    // Using match to handle the Result
    let mut file = match file_result {
        Ok(file) => file,
        Err(error) => {
            println!("Error opening file: {}", error);
            return;
        }
    };

    // Now we read the file contents into a String
    let mut contents = String::new();

    // Reading from the file also returns a Result
    match file.read_to_string(&mut contents) {
        Ok(_) => println!("File contents:\n{}", contents),
        Err(error) => println!("Error reading file: {}", error),
    }

    // ========================
    // SHORTCUT METHODS
    // ========================

    // The `unwrap` method gets the value inside Result if it's Ok.
    // If it's Err, it panics.
    // let file = File::open("missing.txt").unwrap(); // This panics if the file is not found

    // The `expect` method works like unwrap, but with a custom error message
    // let file = File::open("missing.txt").expect("Failed to open file"); // Also panics

    // ========================
    // PROPAGATING ERRORS
    // ========================

    // In real apps, we often want to return errors to the caller instead of handling them all in main.
    // We do this with the `?` operator and functions that return Result<T, E>

    // Here's an example of a function that returns Result
    fn read_file() -> Result<String, io::Error> {
        let mut file = File::open("hello.txt")?; // ? returns early if error occurs
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    // Calling the function
    match read_file() {
        Ok(data) => println!("File read using function:\n{}", data),
        Err(error) => println!("Error: {}", error),
    }

    // ========================
    // CUSTOM ERROR TYPES
    // ========================

    // You can define your own error types using enums
    #[derive(Debug)]
    enum MyError {
        IoError(io::Error),
        CustomError(String),
    }

    impl From<io::Error> for MyError {
        fn from(error: io::Error) -> MyError {
            MyError::IoError(error)
        }
    }

    // Example function that returns a custom error
    fn custom_function() -> Result<(), MyError> {
        let mut file = File::open("hello.txt").map_err(MyError::from)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(MyError::from)?;
        println!("File contents using custom error:\n{}", contents);
        Ok(())
    }

    // Calling the function
    match custom_function() {
        Ok(_) => println!("Custom function executed successfully"),
        Err(error) => println!("Custom error: {:?}", error),
    }
}
