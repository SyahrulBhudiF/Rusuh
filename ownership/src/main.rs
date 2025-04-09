fn main() {
    // Ownership is a core concept in Rust that ensures memory safety without needing a garbage collector.
    // It is based on three main principles: ownership, borrowing, and lifetimes.

    // 1. Ownership: Each value in Rust has a single owner, which is the variable that holds it.
    // When the owner goes out of scope, the value is dropped and memory is freed.

    let s1 = String::from("Hello, Rust!"); // s1 owns the string
    println!("{}", s1); // s1 is still valid here
    // s1 goes out of scope here, and the memory is freed

    // 2. Borrowing: Instead of transferring ownership, you can borrow a value using references.
    // Borrowing allows you to use a value without taking ownership of it.

    let s2 = String::from("Hello, Borrowing!"); // s2 owns the string
    let len = calculate_length(&s2); // &s2 borrows the string
    println!("The length of '{}' is {}.", s2, len); // s2 is still valid here
    // s2 goes out of scope here, and the memory is freed

    // 3. Lifetimes: Lifetimes are a way to specify how long references are valid.
    // They ensure that references do not outlive the data they point to, preventing dangling references.
    // Lifetimes are usually inferred by the compiler, but you can specify them explicitly if needed.
    // Example of explicit lifetimes:
    // fn longest<'a>(s1: &'a str, s2: &'a str) -> &'a str {
    //     if s1.len() > s2.len() {
    //         s1
    //     } else {
    //         s2
    //     }
    // }

    // In this example, the function longest takes two string slices with the same lifetime 'a'
    // and returns a string slice with the same lifetime 'a. This ensures that the returned reference
    // does not outlive either of the input references.
    // The Rust compiler uses a borrow checker to enforce these rules at compile time, ensuring memory safety.
    // The borrow checker checks that:
    // - References do not outlive the data they point to
    // - Mutable references are not aliased (i.e., you cannot have multiple mutable references to the same data at the same time)
    // - Immutable references can coexist with mutable references, but not with each other

    // Example of mutable and immutable references:
    let mut s3 = String::from("Hello, Mutable!"); // s3 owns the string
    let r1 = &s3; // r1 is an immutable reference to s3
    println!("r1: {}", r1); // r1 is valid here
    // let r2 = &mut s3; // This would cause a compile-time error because r1 is still in scope
    // println!("r2: {}", r2); // This would also cause a compile-time error because r2 is a mutable reference
    // To fix this, you can either drop r1 or create a new scope:
    {
        let r2 = &mut s3; // r2 is a mutable reference to s3
        println!("r2: {}", r2); // r2 is valid here
    } // r2 goes out of scope here, and s3 can be used again

    // Example of using a mutable reference:
    let mut s4 = String::from("Hello, Mutable Reference!"); // s4 owns the string
    let r3 = &mut s4; // r3 is a mutable reference to s4
    r3.push_str(" Modified!"); // Modify the string through the mutable reference
    println!("s4: {}", s4); // s4 is modified here
    // r3 goes out of scope here, and s4 can be used again
}

// Function to calculate the length of a string
// Takes a reference to a String as an argument
// Returns the length of the string
fn calculate_length(s: &String) -> usize {
    s.len() // s is a reference to the string, so we can use it without taking ownership
}
