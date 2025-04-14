use std::collections::HashMap;

fn main() {
    // HashMap is a collection of key-value pairs.
    // It is similar to a dictionary in Python or an object in JavaScript.

    // HashMap is a collection of key-value pairs that are stored in a hash table.

    // Example of creating a HashMap
    let mut map = HashMap::new();
    // Adding key-value pairs to the HashMap
    map.insert("key1", "value1");
    map.insert("key2", "value2");

    // Accessing values in the HashMap
    if let Some(value) = map.get("key1") {
        println!("The value for key1 is: {}", value);
    } else {
        println!("Key not found");
    }

    // Iterating over the HashMap
    for (key, value) in &map {
        println!("Key: {}, Value: {}", key, value);
    }

    // HashMap can also be created with a specific capacity
    let mut map2: HashMap<String, String> = HashMap::with_capacity(10);
    // This means that the HashMap can hold 10 key-value pairs without reallocating memory.
    // However, the HashMap is still empty at this point.
    println!("The length of map2 is: {}", map2.len());

    // We can add key-value pairs to map2
    map2.insert(String::from("key1"), String::from("value1"));

    // Now the length of map2 is 1
    println!("The length of map2 is: {}", map2.len());

    // We can also create a HashMap with a specific type
    let mut map3: HashMap<String, String> = HashMap::new();
    map3.insert(String::from("key1"), String::from("value1"));
    map3.insert(String::from("key2"), String::from("value2"));

    // We can also create a HashMap with a integer keys and string values
    let mut map4: HashMap<i32, String> = HashMap::new();
    map4.insert(1, String::from("value1"));
    map4.insert(2, String::from("value2"));

    // We can also create a HashMap with a string keys and integer values
    let mut map5: HashMap<String, i32> = HashMap::new();
    map5.insert(String::from("key1"), 1);
    map5.insert(String::from("key2"), 2);

    // We can also create a HashMap with a string keys and boolean values
    let mut map6: HashMap<String, bool> = HashMap::new();

    // We can also create a HashMap with a string keys and float values
    let mut map7: HashMap<String, f32> = HashMap::new();

    // We can also create a HashMap with a string keys and double values
    let mut map8: HashMap<String, f64> = HashMap::new();

    // We can also create a HashMap with a string keys and char values
    let mut map9: HashMap<String, char> = HashMap::new();

    // We can also create a HashMap with a Struct keys and string values
    #[derive(Hash, Eq, PartialEq)]
    struct MyStruct {
        id: i32,
        name: String,
    }

    let mut map10: HashMap<MyStruct, String> = HashMap::new();
    map10.insert(
        MyStruct {
            id: 1,
            name: String::from("key1"),
        },
        String::from("value1"),
    );

    // We can also create a HashMap with a Enum keys and string values
    #[derive(Hash, Eq, PartialEq)]
    enum MyEnum {
        Variant1,
        Variant2,
    }
    let mut map11: HashMap<MyEnum, String> = HashMap::new();
    map11.insert(MyEnum::Variant1, String::from("value1"));

    // We can also create a HashMap with a string keys and vector values
    let mut map12: HashMap<String, Vec<String>> = HashMap::new();
    map12.insert(String::from("key1"), Vec::<String>::new());
}
