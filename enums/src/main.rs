// Enums in Rust

// Enums are a powerful way to define a type that can have multiple different values.
// Enums are defined using the `enum` keyword, followed by the name of the enum and its variants in curly braces.
// Enums can also have associated data, which allows you to define a type that can have different values depending on the variant.
// Enums are a powerful way to define a type that can have multiple different values, and they are used extensively in Rust.

// Example of an enum
#[derive(Debug)] // Deriving the Debug trait allows us to print the enum using {:?}
enum IpAddr {
    V4(u8, u8, u8, u8), // IPv4 address represented as four u8 values
    V6(String),         // IPv6 address represented as a String
}

// Example of an enum with associated data
#[derive(Debug)] // Deriving the Debug trait allows us to print the enum using {:?}
enum Message {
    Quit,                       // No data
    Move { x: i32, y: i32 },    // Struct-like variant with named fields
    Write(String),              // Tuple-like variant with a String
    ChangeColor(i32, i32, i32), // Tuple-like variant with three i32 values
}

// Example of an enum with struct
#[derive(Debug)] // Deriving the Debug trait allows us to print the struct using {:?}
struct QuitMessage;
#[derive(Debug)] // Deriving the Debug trait allows us to print the struct using {:?}
enum MoveMessage {
    Quit(QuitMessage),       // Struct-like variant with a QuitMessage
    Move { x: i32, y: i32 }, // Struct-like variant with named fields
}

// Example of an enum with a method
impl Message {
    fn call(&self) {
        // Method to call the enum
        println!("Message called");
    }
}

enum Coin {
    Penny,
    Nickel,
    Dime,
    Quarter,
}

// Example method for the Coin enum
impl Coin {
    fn value_in_cents(&self) -> u8 {
        match self {
            Coin::Penny => 1,
            Coin::Nickel => 5,
            Coin::Dime => 10,
            Coin::Quarter => 25,
        }
    }
}

// Matching with optional values
fn plus_one(x: Option<i32>) -> Option<i32> {
    match x {
        Some(i) => Some(i + 1), // If x is Some, return Some(i + 1)
        None => None,           // If x is None, return None
    }
}

fn main() {
    // Creating an instance of the enum
    let home = IpAddr::V4(127, 0, 0, 1); // Creating an IPv4 address
    let loopback = IpAddr::V6(String::from("::1")); // Creating an IPv6 address

    // Printing the enum instances
    println!("Home IP: {:?}", home); // Using the Debug trait to print the enum
    println!("Loopback IP: {:?}", loopback); // Using the Debug trait to print the enum

    // Creating an instance of the enum with associated data
    let message = Message::Move { x: 10, y: 20 }; // Creating a Move message with x and y coordinates
    println!("Message: {:?}", message); // Using the Debug trait to print the enum

    // Calling the method on the enum
    message.call(); // Calling the method on the enum instance

    // Creating an instance of the enum with struct
    let quit_message = QuitMessage; // Creating a Quit message
    let move_message = MoveMessage::Move { x: 10, y: 20 }; // Creating a Move message with x and y coordinates
    println!("Quit Message: {:?}", quit_message); // Using the Debug trait to print the enum
    println!("Move Message: {:?}", move_message); // Using the Debug trait to print the enum

    // Using the enum with a match statement
    match message {
        Message::Quit => println!("Quit message"),
        Message::Move { x, y } => println!("Move message with x: {}, y: {}", x, y),
        Message::Write(text) => println!("Write message with text: {}", text),
        Message::ChangeColor(r, g, b) => println!("Change color to r: {}, g: {}, b: {}", r, g, b),
    }

    // Using the enum with a match statement
    match move_message {
        MoveMessage::Quit(quit) => println!("Quit message with quit: {:?}", quit),
        MoveMessage::Move { x, y } => println!("Move message with x: {}, y: {}", x, y),
    }

    // Using the enum with a match statement
    match home {
        IpAddr::V4(a, b, c, d) => println!("IPv4 address: {}.{}.{}.{}", a, b, c, d),
        IpAddr::V6(addr) => println!("IPv6 address: {}", addr),
    }

    // Using the enum with a match statement
    match loopback {
        IpAddr::V4(a, b, c, d) => println!("IPv4 address: {}.{}.{}.{}", a, b, c, d),
        IpAddr::V6(addr) => println!("IPv6 address: {}", addr),
    }

    // Creating an instance of the Coin enum
    let coin = Coin::Quarter; // Creating a Quarter coin
    println!("Coin value in cents: {}", coin.value_in_cents()); // Using the method to get the value in cents

    // Using the plus_one function with an Option
    let x = Some(5); // Creating an Option with a value
    let y = plus_one(x); // Calling the plus_one function with the Option
    println!("Plus one: {:?}", y); // Printing the result of the plus_one function

    // Using the plus_one function with a None value
    let x = None; // Creating an Option with no value
    let y = plus_one(x); // Calling the plus_one function with the Option
    println!("Plus one: {:?}", y); // Printing the result of the plus_one function

    // Example of using if let\
    let some_value = Some(5);
    if let Some(x) = some_value {
        println!("The value is: {}", x); // If some_value is Some, print the value
    } else {
        println!("No value found"); // If some_value is None, print a message
    }
}
