fn main() {
    // Variables are immutable by default
    let mut x = 5; // mut is used to make a variable mutable
    println!("The value of x is: {x}");

    x = 6; // This is allowed because x is mutable
    println!("The value of x is: {x}");

    // Constants are always immutable
    const THREE_HOURS_IN_SECONDS: u32 = 60 * 60 * 3; // Constants are always uppercase

    // Shadowing allows you to change the type of variable and reuse the same name
    let x = 5; // This is the first x
    let x = x + 1; // This is the second x, and it is a different variable cause the first x is shadowed
    {
        let x = x * 2; // This is the third x, and it is a different variable cause the second x is shadowed
        println!("The value of x in the inner scope is: {x}"); // This is the third x
    }

    // This is the second x cause the third x is in a different scope
    println!("The value of x is: {x}");

    /**
     * Difference between shadowing and mutability
     */
    // Shadowing allows you to change the type of variable and reuse the same name
    let spaces = "   ";
    let spaces = spaces.len(); // This is allowed because spaces is shadowed
    println!("The value of spaces is: {spaces}");

    // Mutability allows you to change the value of a variable but not the type and you can't reuse the same name
    let mut spaces = "   ";
    // spaces = spaces.len(); This is not allowed because spaces is not shadowed
}
