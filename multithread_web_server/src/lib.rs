use std::sync::{Arc, Mutex, mpsc};
use std::thread;

// Type alias untuk pekerjaan yang akan dijalankan oleh thread
type Job = Box<dyn FnOnce() + Send + 'static>;

/// Struct utama ThreadPool
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}

/// Error yang dikembalikan jika pembuatan ThreadPool gagal
#[derive(Debug)]
pub struct PoolCreationError;

impl PoolCreationError {
    pub fn new() -> PoolCreationError {
        panic!("PoolCreationError")
    }
}

impl ThreadPool {
    /// Membuat ThreadPool baru dengan ukuran tertentu
    pub fn new(size: usize) -> Result<ThreadPool, PoolCreationError> {
        if size == 0 {
            return Err(PoolCreationError::new());
        }

        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        Ok(ThreadPool { workers, sender: Some(sender), })
    }

    /// Menjalankan sebuah job di thread pool
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.as_ref().unwrap().send(job).unwrap();
    }

    fn drop(&mut self) {
        drop(self.sender.take());

        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);

            worker.thread.join().unwrap();
        }
    }
}

/// Representasi sebuah thread worker dalam thread pool
struct Worker {
    id: usize,
    thread: thread::JoinHandle<()>,
}

impl Worker {
    /// Membuat worker baru yang langsung mulai menunggu job
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv();

            match message {
                Ok(job) => {
                    println!("Worker {id} got a job; executing.");

                    job();
                }
                Err(_) => {
                    println!("Worker {id} disconnected; shutting down.");
                    break;
                }
            }
        });

        Worker { id, thread }
    }
}
