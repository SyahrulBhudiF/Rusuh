fn main() {
    println!("Hello, world!");

    another_function(); // This is a function call
    another_function2(5, 6.4, 1); // This is a function call

    // Statements are instructions that perform some action and do not return a value, and they end in a semicolon
    let y = 6; // This is a statement

    // Expressions evaluate to a resulting value, and they do not end in a semicolon
    let x = {
        let y = 3;
        y + 1 // This is an expression that evaluates to 4 because it is the last line in the block and it does not end in a semicolon, if it ended in a semicolon, it would be a statement
    };

    let x = another_function3(); // This is a function call
    println!("The value of x is: {}", x);

    let x = another_function4(5, 6); // This is a function call
    println!("The value of x is: {}", x);
}

// Functions are declared using the fn keyword, followed by the function name, and then the parameter list in parentheses, and then the return type, and then the function body in curly braces
fn another_function() {
    println!("Another function.");
}

// Functions can take parameters, and the parameter list is a comma-separated list of parameter names and types
fn another_function2(x: i32, y: f64, z: u8) {
    println!("The value of x is: {x}");
    println!("The value of y is: {y}");
    println!("The value of z is: {z}");
}

// Functions can return values using the return keyword followed by the value to return from the function
fn another_function3() -> i32 {
    5 // This is an expression that evaluates to 5 because it is the last line in the function, and it does not end in a semicolon, if it ended in a semicolon, it would be a statement
}

// Functions can return values using the return keyword followed by the value to return from the function
fn another_function4(x: i32, y: i32) -> i32 {
    x + y // This is an expression that evaluates to the sum of x and y because it is the last line in the function, and it does not end in a semicolon, if it ended in a semicolon, it would be a statement
}

// Error case: fn another_function5() -> i32 {
//     5; // This is a statement that does not return a value, and it ends in a semicolon, so it is an error
// }

// mismatched types is an error that occurs when the types of values do not match the expected types in the program
