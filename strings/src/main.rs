fn main() {
    // String is a collection of characters that can grow or shrink in size.
    // It is a contiguous growable array type in Rust.

    // Strings are stored on the heap, which means they can grow and shrink in size.
    // They are similar to arrays, but arrays have a fixed size.
    // Example of creating a string
    let mut s = String::new();

    // Adding characters to the string
    s.push('H');
    s.push('e');

    // Accessing characters in the string
    println!("The first character is: {}", s.chars().nth(0).unwrap());
    println!("The second character is: {}", s.chars().nth(1).unwrap());

    // String also can be created with a macro
    let s2 = String::from("Hello, World!");

    // Iterating over the string
    for c in s2.chars() {
        println!("Character: {}", c);
    }

    // String can also be created with a specific capacity
    let mut s3 = String::with_capacity(10);
    // This means that the string can hold 10 characters without reallocating memory.
    // However, the string is still empty at this point.
    println!("The length of s3 is: {}", s3.len());

    // We can add characters to s3
    s3.push('H');
    s3.push('e');

    // Now the length of s3 is 2

    println!("The length of s3 is: {}", s3.len());

    // We can also create a string with a specific type
    let mut s4: String = String::new();
    s4.push('H');
    s4.push('e');

    // We can also create a string with a specific type and initial values
    let s5: String = String::from("Hello, World!");

    // We can also create a string of characters
    let mut s6: String = String::new();
    s6.push('H');
    s6.push('e');

    // String also represents a sequence of bytes, so we can create a string from a byte array
    let byte_array: &[u8] = b"Hello, World!";
    let s7 = String::from_utf8_lossy(byte_array);
    println!("String from byte array: {}", s7);

    // String can also be created from a slice
    let slice: &str = "Hello, World!";
    let s8 = String::from(slice);
    println!("String from slice: {}", s8);

    // String can also be created from a vector of bytes
    let byte_vector: Vec<u8> = vec![72, 101, 108, 108, 111, 44, 32, 87, 111, 114, 108, 100, 33];
    let s9 = String::from_utf8(byte_vector).unwrap();
    println!("String from vector of bytes: {}", s9);

    // String can also be created from a vector of characters
    let char_vector: Vec<char> = vec![
        'H', 'e', 'l', 'l', 'o', ',', ' ', 'W', 'o', 'r', 'l', 'd', '!',
    ];
    let s10: String = char_vector.iter().collect();
    println!("String from vector of characters: {}", s10);

    // String can also be created from a vector of strings
    let string_vector: Vec<String> = vec![
        String::from("Hello"),
        String::from(", "),
        String::from("World"),
    ];
    let s11: String = string_vector.concat();
    println!("String from vector of strings: {}", s11);

    // String can also be created from a vector of tuples
    let tuple_vector: Vec<(i32, String)> = vec![
        (1, String::from("Hello")),
        (2, String::from(", ")),
        (3, String::from("World")),
    ];
    let s12: String = tuple_vector
        .iter()
        .map(|(_, s)| s.as_str())
        .collect::<Vec<&str>>()
        .concat();
    println!("String from vector of tuples: {}", s12);

    // String type is a UTF-8 encoded string, which means it can represent any Unicode character.
    // This means that we can create a string from any Unicode character.
    let unicode_string = String::from("Hello, 世界!");
    println!("Unicode string: {}", unicode_string);

    // String can also be created from a vector of Unicode characters
    let unicode_char_vector: Vec<char> = vec!['H', 'e', 'l', 'l', 'o', ',', ' ', '世', '界', '!'];
    let unicode_string2: String = unicode_char_vector.iter().collect();
    println!(
        "Unicode string from vector of characters: {}",
        unicode_string2
    );

    // String can also return a byte array
    let byte_array2: &[u8] = s.as_bytes();
    println!("Byte array: {:?}", byte_array2);

    // String can also return a slice
    let slice2: &str = &s;
    println!("Slice: {}", slice2);

    // String can also return a vector of bytes
    let byte_vector2: Vec<u8> = s.into_bytes();
    println!("Vector of bytes: {:?}", byte_vector2);

    // So String is a powerful type in Rust that can represent any sequence of characters.
}
