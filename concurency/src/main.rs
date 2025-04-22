use std::sync::{Mutex, mpsc, Arc};
use std::thread;

fn main() {
    let v = vec![1, 2, 3, 4, 5];

    let handle = thread::spawn(move || {
        for i in 1..10 {
            println!("hi {} for spawned thread and this vector {}", i, v[i % 5]);
            thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    for i in 1..5 {
        println!("hi {} from main thread", i);
        thread::sleep(std::time::Duration::from_millis(1));
    }

    handle.join().unwrap();

    let (tx, rx) = mpsc::channel();

    let tx1 = tx.clone();
    thread::spawn(move || {
        let vals = vec![1, 2, 3, 4, 5];
        for val in vals {
            tx.send(val).unwrap();
            thread::sleep(std::time::Duration::from_millis(1));
        }
    })
    .join()
    .unwrap();

    thread::spawn(move || {
        let vals = vec![6, 7, 8, 9, 10];
        for val in vals {
            tx1.send(val).unwrap();
            thread::sleep(std::time::Duration::from_millis(1));
        }
    })
    .join()
    .unwrap();

    for received in rx {
        println!("Got: {}", received);
    }

    let m = Mutex::new(5);

    {
        let mut num = m.lock().unwrap();
        *num += 1;
    }

    println!("m = {:?}", m);

    let counter = Arc::new(Mutex::new(0));
    let mut handles = vec![];

    for _ in 0..10 {
        let counter = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            let mut num = counter.lock().unwrap();

            *num += 1;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    println!("Result: {}", *counter.lock().unwrap());
}
