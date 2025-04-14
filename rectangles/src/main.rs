#[derive(Debug)] // Deriving the Debug trait allows us to print the struct using {:?}
struct Rectangle {
    width: u32,
    height: u32,
}

impl Rectangle {
    // Implementing methods for the Rectangle struct
    fn area(&self) -> u32 {
        self.width * self.height
    }

    // Method to check if the rectangle can hold another rectangle
    fn can_hold(&self, other: &Rectangle) -> bool {
        self.width > other.width && self.height > other.height
    }

    // Method to create a new rectangle with the same width and height
    fn square(size: u32) -> Self {
        Self {
            width: size,
            height: size,
        }
    }
}

fn main() {
    // This program calculates the area of a rectangle given its width and height.
    // The area is calculated using the formula: area = width * height.

    let width1 = 30;
    let height1 = 50;

    println!(
        "The area of the rectangle is {} square pixels.",
        area(width1, height1)
    );

    // Refactored code to use a tuple to hold the dimensions of the rectangle

    let rect2 = (30, 50);

    println!(
        "The area of the rectangle is {} square pixels.",
        area_tup(rect2)
    );

    // Refactored code to use a struct to hold the dimensions of the rectangle
    let rect3 = Rectangle {
        width: 30,
        height: 50,
    };

    println!(
        "The area of the rectangle is {} square pixels.",
        area_struct(&rect3)
    );

    // Using the struct to print the dimensions of the rectangle
    println!("rect3 is {:#?}", rect3); // Using the Debug trait to print the struct
    dbg!(&rect3); // Using the dbg! macro to print the struct and its dimensions

    // Defining methods for the struct
    let rect4 = Rectangle {
        width: 30,
        height: 50,
    };

    println!(
        "The area of the rectangle is {} square pixels.",
        rect4.area()
    );

    // Using the can_hold method to check if one rectangle can hold another
    println!("Can rect4 hold rect3? {}", rect4.can_hold(&rect3));

    // Creating a square using the square method
    let square = Rectangle::square(10);
    println!(
        "The area of the square is {} square pixels.",
        square.area()
    );
}

fn area_struct(rectangle: &Rectangle) -> u32 {
    rectangle.width * rectangle.height
}

// Function to calculate the area of a rectangle given its width and height
fn area(width: u32, height: u32) -> u32 {
    width * height
}

// Function to calculate the area of a rectangle given its dimensions as a tuple
fn area_tup(dimensions: (u32, u32)) -> u32 {
    dimensions.0 * dimensions.1
}
