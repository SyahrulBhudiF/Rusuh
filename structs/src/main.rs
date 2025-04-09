// Structs are custom data types that let you package together related data.
// They are similar to tuples, but they have named fields, which makes them more readable and easier to work with.

// Structs are used to create complex data types that can hold multiple values of different types.

// Structs are defined using the `struct` keyword, followed by the name of the struct and its fields in curly braces.

// The fields of a struct can be of any type, including other structs, enums, and primitive types.

// Structs can also have methods associated with them, which are similar to functions but are defined within the context of the struct.

// Structs are a powerful way to create complex data types in Rust, and they are used extensively in the language.

// Example of a struct
struct User {
    active: bool,
    username: String,
    email: String,
    sign_in_count: u64,
}

fn main() {
    // Creating an instance of a struct, using the struct name and the field names
    let user1 = User {
        active: true,
        username: String::from("user1"),
        email: String::from("user1@gmail.com"),
        sign_in_count: 1,
    };

    // Accessing the fields of a struct
    println!("User1: {} - {}", user1.username, user1.email); // If you want to print the username and email of user1

    // Creating a mutable instance of a struct
    let mut user2 = User {
        active: true,
        username: String::from("user2"),
        email: String::from("user2@gmail.com"),
        sign_in_count: 1,
    };

    // Modifying the fields of a mutable struct
    user2.email = String::from("user24w@gmail.com");
    println!("User2: {} - {}", user2.username, user2.email); // If you want to print the username and email of user2

    // Creating a new instance of a struct using the `build_user` function
    let user3 = build_user(String::from("user3"), String::from("user3@gmail.com"));
    println!("User3: {} - {}", user3.username, user3.email); // If you want to print the username and email of user3

    // Creating a instance of a struct using the other struct
    let user4 = build_user(user3.email, String::from("user4@gmail.com"));
    println!("User4: {} - {}", user4.username, user4.email); // If you want to print the username and email of user4

    // Creating a new instance of a struct using the `..` syntax to copy fields from another instance
    let user5 = User {
        email: String::from("user5@gmail.com"),
        ..user4
    };
    println!("User5: {} - {}", user5.username, user5.email); // If you want to print the username and email of user5

    // Tuple Structs
    // Are similar to regular structs, but they do not have named fields.
    // Instead, they are defined using a tuple-like syntax, with the types of the fields specified in parentheses.
    // Tuple structs are useful when you want to create a simple data type that does not require named fields.
    // Example of a tuple struct
    struct Color(u8, u8, u8);
    struct Point(u8, u8, u8);

    // Creating an instance of a tuple struct
    let black = Color(0, 0, 0);
    let origin = Point(0, 0, 0);

    let Point(0, 0, ..) = origin; // Destructuring the tuple struct to get the values of the fields
    println!("Black: ({}, {}, {})", black.0, black.1, black.2); // If you want to print the color of black
    println!("Origin: ({}, {}, {})", origin.0, origin.1, origin.2); // If you want to print the point of origin

    // Unit-like Structs
    // Are similar to regular structs, but they do not have any fields.
    // They are useful when you want to create a simple data type that does not require any fields.

    // Example of a unit-like struct
    struct AlwaysEqual;
    struct UnitLikeStruct;

    // Creating an instance of a unit-like struct
    let unit_like_struct = UnitLikeStruct;
    let always_equal = AlwaysEqual;

    // Unit-like structs are useful when you want to create a simple data type that does not require any fields.
    // They can be used to create a type that has no data but can still be used as a type.
    // For example, you can use a unit-like struct to create a type that represents a specific state or condition.
    // Unit-like structs can also be used to create a type that has no data but can still be used as a type.
    // Build Structs
}

// Function to create a new instance of a struct
fn build_user(email: String, username: String) -> User {
    User {
        email,
        username,
        active: true,
        sign_in_count: 1,
    }
}
