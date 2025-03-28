fn main() {
    /**
     * Data types
     */
    // Rust is a statically typed language, which means that it must know the types of all variables at compile time
    let spaces = "   "; // This is a string literal
    let spaces: &str = "   "; // This is a string slice

    // Scalar types represent a single value and there are four primary scalar types: integers, floating-point numbers, Booleans, and characters

    // Integers are numbers without a fractional component
    // There are signed and unsigned integers, and they can be 8, 16, 32, 64, or 128 bits
    // Signed integers can store positive and negative numbers, while unsigned integers can only store positive numbers
    let integer: i8 = 127; // i8 is a signed 8-bit integer
    let integer: u8 = 255; // u8 is an unsigned 8-bit integer
    let integer: isize = -126; // isize is a signed integer that is the same size as the machine's word size
    let integer: usize = 126; // usize is an unsigned integer that is the same size as the machine's word size

    // Integer overflow is when a number is too large to store in the variable's data type
    // Rust has a feature called "panic on overflow" that will cause the program to crash if an integer overflows
    // To avoid this, you can use the wrapping_add, wrapping_sub, wrapping_mul, wrapping_div, and wrapping_rem methods

    // error case 1: let integer = 255u8 + 1; // This will panic
    // error case 2: let integer = 255u8 - 1; // This will panic
    // this will not panic
    let integer = 255u8;
    let integer = integer.wrapping_add(1); // This will not panic, it will wrap around to 0

    // Floating-point numbers are numbers with a fractional component, and they can be f32 or f64
    let float: f32 = 3.14; // f32 is a 32-bit floating-point number
    let float: f64 = 3.14; // f64 is a 64-bit floating-point number

    // Numeric operations are performed using the standard arithmetic operators
    let sum = 5 + 10; // Addition
    let difference = 95.5 - 4.3; // Subtraction
    let product = 4 * 30; // Multiplication
    let quotient = 56.7 / 32.2; // Division
    let remainder = 43 % 5; // Remainder

    // Booleans are either true or false and they are used for logical operations
    let boolean: bool = true; // true is a boolean literal
    let boolean: bool = false; // false is a boolean literal

    // The Character type represents a single Unicode scalar value, and it is denoted by single quotes
    let character: char = 'A'; // A is a character literal
    let character: char = '😻'; // 😻 is a character literal

    // Compound types can group multiple values into one type and there are two primary compound types: tuples and arrays

    // Tuples are a fixed-size collection of values, and they can have different types, and they are denoted by parentheses
    let tuple: (i32, f64, u8) = (500, 6.4, 1); // This is a tuple with an i32, f64, and u8

    // You can destructure a tuple to get its individual values
    let (x, y, z) = tuple;
    println!("The value of x is: {x}");
    println!("The value of y is: {y}");
    println!("The value of z is: {z}");

    // You can also access a tuple's individual values using dot notation and zero-based indexing
    let x = tuple.0; // This is the first value in the tuple
    let y = tuple.1; // This is the second value in the tuple
    let z = tuple.2; // This is the third value in the tuple

    // Arrays are a fixed-size collection of values, and they must have the same type, and they are denoted by square brackets
    let array: [i32; 5] = [1, 2, 3, 4, 5]; // This is an array with 5 i32 values
    let array = [3; 5]; // This is an array with 5 i32 values that are all 3
    let array = [1, 2, 3, 4, 5]; // This is an array with 5 i32 values

    // You can access an array's individual values using zero-based indexing
    let first = array[0]; // This is the first value in the array
    let second = array[1]; // This is the second value in the array

    // You can access an array's individual values using the get method, which returns an Option
    let first = array.get(0); // This is the first value in the array
    let second = array.get(1); // This is the second value in the array

    // You can access an array's individual values using pattern matching
    match array {
        [first, second, ..] => {
            println!("The first value is: {first}");
            println!("The second value is: {second}");
        }
    }

    // You can access an array's individual values using a for loop
    for element in array.iter() {
        println!("The value is: {element}");
    }

    // You can access an array's individual values using a for loop with an index
    for (index, element) in array.iter().enumerate() {
        println!("The index is: {index}");
        println!("The value is: {element}");
    }

    // Invalid array access will cause a panic at runtime
    // error case 1: let value = array[10]; // This will panic
    // error case 2: let value = array.get(10); // This will not panic, it will return None
    // error case 3: match array {
    //     [first, second, .., tenth] => {
    //         println!("The first value is: {first}");
    //         println!("The second value is: {second}");
    //         println!("The tenth value is: {tenth}");
    //     }
    // }
}
