use std::f64::consts;

fn main() {
    // Traits at rust is a way to define shared behavior between different types.
    // They allow you to specify a set of methods that a type must implement, enabling polymorphism and code reuse.
    // Traits are similar to interfaces in other programming languages.

    // You can define a trait using the `trait` keyword, and then implement it for specific types.
    trait Shape {
        fn area(&self) -> f64;
    }

    // Implementing the trait for a struct
    struct Circle {
        radius: f64,
    }

    impl Shape for Circle {
        fn area(&self) -> f64 {
            consts::PI * self.radius * self.radius
        }
    }

    // Implementing the trait for another struct
    struct Rectangle {
        width: f64,
        height: f64,
    }

    impl Shape for Rectangle {
        fn area(&self) -> f64 {
            self.width * self.height
        }
    }

    // Using the trait
    let circle = Circle { radius: 5.0 };
    let rectangle = Rectangle {
        width: 4.0,
        height: 3.0,
    };

    println!("Circle area: {}", circle.area());
    println!("Rectangle area: {}", rectangle.area());

    // You can also define default implementations for methods in a trait.
    trait ShapeWithDefault {
        fn area(&self) -> f64;

        fn perimeter(&self) -> f64 {
            0.0 // Default implementation
        }
    }

    // Implementing the trait for a struct with a custom perimeter
    struct Square {
        side: f64,
    }

    impl ShapeWithDefault for Square {
        fn area(&self) -> f64 {
            self.side * self.side
        }

        fn perimeter(&self) -> f64 {
            4.0 * self.side
        }
    }

    let square = Square { side: 3.0 };
    println!("Square area: {}", square.area());
    println!("Square perimeter: {}", square.perimeter());

    // Traits can also be used as bounds on generic types, allowing you to specify that a type must implement a certain trait.
    fn print_area<T: Shape>(shape: &T) {
        println!("Area: {}", shape.area());
    }

    print_area(&circle);
    print_area(&rectangle);

    // You can also use trait objects to achieve dynamic dispatch, allowing you to work with different types that implement the same trait.
    let shapes: Vec<Box<dyn Shape>> = vec![
        Box::new(circle),
        Box::new(rectangle),
        Box::new(square),
    ];

    for shape in shapes {
        println!("Shape area: {}", shape.area());
    }

    // Traits are a powerful feature in Rust that enable you to define shared behavior and create flexible, reusable code.
}
