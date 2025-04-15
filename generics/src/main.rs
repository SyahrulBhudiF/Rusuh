fn main() {
    // Generic is a way to write code that can work with any data type.
    // It allows you to create functions, structs, enums, and traits that can operate on different types without needing to specify the exact type in advance.
    // This is useful for creating reusable and flexible code.
    // Generics are often used in Rust to create data structures and functions that can work with any type.

    // For example, you can create a function that takes a generic type T and returns a value of the same type.
    fn generic_function<T>(value: T) -> T {
        value
    }

    // You can also create structs that use generics. For example, you can create a struct that holds a value of any type T.
    struct GenericStruct<T> {
        value: T,
    }

    // You can create an instance of the struct with a specific type.
    let int_instance = GenericStruct { value: 42 };
    let string_instance = GenericStruct { value: String::from("Hello, world!") };

    // You can also create enums that use generics. For example, you can create an enum that can hold a value of any type T.
    enum GenericEnum<T> {
        Value(T),
        None,
    }

    // You can create an instance of the enum with a specific type.
    let int_enum = GenericEnum::Value(42);
    let string_enum = GenericEnum::Value(String::from("Hello, world!"));

    // You can also create traits that use generics. For example, you can create a trait that defines a method that takes a generic type T.
    trait GenericTrait<T> {
        fn do_something(&self, value: T);
    }

    // You can implement the trait for a specific type.
    impl GenericTrait<i32> for GenericStruct<i32> {
        fn do_something(&self, value: i32) {
            println!("Doing something with value: {}", value);
        }
    }

    // You can also use generics with functions that take multiple types. For example, you can create a function that takes two generic types T and U.
    fn generic_function_with_two_types<T, U>(value1: T, value2: U) {
        println!("Value 1: {:?}, Value 2: {:?}", value1, value2);
    }

    // You can call the function with different types.
    generic_function_with_two_types(42, "Hello");

    // With generics, you can create flexible and reusable code that can work with different types without needing to write separate implementations for each type.
    // This makes your code more efficient and easier to maintain.
}
