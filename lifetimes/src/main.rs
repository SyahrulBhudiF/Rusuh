fn main() {
    // Lifetimes are a way to specify how long references should be valid in Rust.
    // They help the compiler ensure that references do not outlive the data they point to, preventing dangling references and memory safety issues.
    // Lifetimes are denoted with an apostrophe (') followed by a name, like 'a, 'b, etc.
    // They are used in function signatures, struct definitions, and trait implementations to indicate the relationship between references and the data they point to.

    // Here's an example of a function that takes two references with the same lifetime and returns a reference with the same lifetime.
    fn longest<'a>(s1: &'a str, s2: &'a str) -> &'a str {
        if s1.len() > s2.len() { s1 } else { s2 }
    }
    // In this example, the function longest takes two string slices (references to strings) with the same lifetime 'a and returns a reference to the longer string slice.
    // The lifetime 'a indicates that the returned reference will be valid as long as both input references are valid.
    // This prevents returning a reference to a local variable that goes out of scope, which would lead to a dangling reference.

    // Here's an example of a struct with a lifetime parameter.
    struct ImportantExcerpt<'a> {
        part: &'a str,
    }

    // In this example, the struct ImportantExcerpt has a lifetime parameter 'a that indicates the lifetime of the reference part.
    // This means that the reference part will be valid as long as the ImportantExcerpt instance is valid.
    // This ensures that the reference does not outlive the data it points to, preventing dangling references.
    // You can create an instance of the struct with a specific lifetime.
    let s: String = String::from("Hello, world!");
    let excerpt: ImportantExcerpt = ImportantExcerpt { part: &s };

    // The reference part will be valid as long as the string s is valid.

    // Lifetimes are a powerful feature of Rust that help ensure memory safety and prevent dangling references.
    // They allow you to specify how long references should be valid and ensure that references do not outlive the data they point to.
    // This prevents memory safety issues and makes your code more reliable.
    // Lifetimes can be complex, but they are an essential part of Rust's ownership system and help ensure that your code is safe and efficient.
    // Here's an example of a function that takes a reference with a specific lifetime and returns a reference with a different lifetime.
    fn longest_with_different_lifetime<'a, 'b>(s1: &'a str, s2: &'b str) -> &'a str {
        if s1.len() > s2.len() { s1 } else { s2 }
    }
}
