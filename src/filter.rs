use std::collections::HashMap;
use simple_dns;

struct MaybeContainer {
    container: Option<Box<Container>>
}

type Container = HashMap<String, MaybeContainer>;

struct Filter {
    root: Container
}

impl Filter {
    pub fn new() -> Self {
        Filter {
            root: HashMap::new()
        }
    }
    pub fn add(&mut self, element: Vec<String>) {
        let mut cur = &mut(self.root);
        for segment in element.iter().rev() {
            if !cur.contains_key(segment) {
                cur.insert(segment.clone(), MaybeContainer{container: Some(Box::new(Container::new()))});
            }
            cur = cur.get_mut(segment).unwrap().container.unwrap();
        }
    }
}
