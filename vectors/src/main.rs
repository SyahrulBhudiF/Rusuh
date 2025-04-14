fn main() {
    // Vector is a collection of elements of the same type that can grow or shrink in size.
    // It is a contiguous growable array type in Rust.

    // Vectors are stored on the heap, which means they can grow and shrink in size.
    // They are similar to arrays, but arrays have a fixed size.

    // Example of creating a vector
    let mut vec = Vec::new();

    // Adding elements to the vector
    vec.push(1);
    vec.push(2);
    vec.push(3);

    // Accessing elements in the vector
    println!("The first element is: {}", vec[0]);
    println!("The second element is: {}", vec[1]);
    println!("The third element is: {}", vec[2]);

    // Vector also can be created with a macro
    let vec2 = vec![1, 2, 3, 4, 5];

    // Iterating over the vector
    for i in &vec2 {
        println!("Element: {}", i);
    }

    // Vector can also be created with a specific capacity
    let mut vec3 = Vec::with_capacity(10);
    // This means that the vector can hold 10 elements without reallocating memory.

    // However, the vector is still empty at this point.
    println!("The length of vec3 is: {}", vec3.len());

    // We can add elements to vec3
    vec3.push(1);
    vec3.push(2);

    // Now the length of vec3 is 2
    println!("The length of vec3 is: {}", vec3.len());

    // We can also create a vector with a specific type
    let mut vec4: Vec<i32> = Vec::new();
    vec4.push(1);
    vec4.push(2);

    // We can also create a vector with a specific type and initial values
    let vec5: Vec<i32> = vec![1, 2, 3, 4, 5];

    // We can also create a vector of strings
    let mut vec6: Vec<String> = Vec::new();
    vec6.push(String::from("Hello"));
    vec6.push(String::from("World"));

    // We can also create a vector of characters
    let mut vec7: Vec<char> = Vec::new();
    vec7.push('H');
    vec7.push('e');

    // We can also create a vector of tuples
    let mut vec8: Vec<(i32, String)> = Vec::new();
    vec8.push((1, String::from("Hello")));
    vec8.push((2, String::from("World")));

    // We can also create a vector of structs
    #[derive(Debug)]
    struct Person {
        name: String,
        age: i32,
    }
    let mut vec9: Vec<Person> = Vec::new();

    vec9.push(Person {
        name: String::from("Alice"),
        age: 30,
    });
    vec9.push(Person {
        name: String::from("Bob"),
        age: 25,
    });

    // We can also create a vector of enums
    #[derive(Debug)]
    enum Shape {
        Circle(f64),
        Rectangle(f64, f64),
    }
    let mut vec10: Vec<Shape> = Vec::new();
    vec10.push(Shape::Circle(1.0));
    vec10.push(Shape::Rectangle(2.0, 3.0));

    // We can also create a vector of references
    let s1 = String::from("Hello");
    let s2 = String::from("World");
    let mut vec11: Vec<&String> = Vec::new();
    vec11.push(&s1);
    vec11.push(&s2);

    // We can also create a vector with Options
    let mut vec12: Vec<Option<i32>> = Vec::new();
    vec12.push(Some(1));
    vec12.push(None);

    match vec12.pop() {
        Some(Some(value)) => println!("Popped value: {}", value),
        Some(None) => println!("Popped None"),
        None => println!("Vector is empty"),
    }
}
