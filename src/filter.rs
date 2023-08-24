use std::fs::{read_to_string};
use std::collections::HashSet;
use std::io;

pub struct Filter {
    names: HashSet<String>
}

impl Filter {
    pub fn new() -> Self {
        return Filter {
            names: HashSet::new()
        }
    }

    pub fn add_file(&mut self, file_path: &str) -> io::Result<()> {
        for line in read_to_string(file_path).expect("Failed reading file").lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut split = line.split(' ');
            split.next();
            if let Some(name) = split.next() {
                //println!("{}", name);
                self.names.insert(name.parse().unwrap());
            }
        }
        Ok(())
    }

    pub fn contains(&self, name: &String) -> bool {
        self.names.contains(name)
    }
}