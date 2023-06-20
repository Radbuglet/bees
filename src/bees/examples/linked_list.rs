use std::iter;

use bees::{Allocation, Struct};

#[derive(Struct)]
pub struct LinkedList<T: 'static> {
    value: T,
    left: Option<LinkedListRef<T>>,
    right: Option<LinkedListRef<T>>,
}

impl<T> LinkedList<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            left: None,
            right: None,
        }
    }
}

impl<T> LinkedListRef<T> {
    pub fn insert_before(self, right: Self) {
        self.set_right(Some(right));
        self.set_left(right.left());
        right.set_left(Some(self));
    }

    pub fn insert_after(self, left: Self) {
        self.set_left(Some(left));
        self.set_right(left.right());
        left.set_right(Some(self));
    }

    pub fn remove(self) {
        if let Some(left) = self.left() {
            left.set_right(self.right());
        }

        if let Some(right) = self.right() {
            right.set_left(self.left());
        }

        self.set_left(None);
        self.set_right(None);
    }

    pub fn iter_right(self) -> impl Iterator<Item = LinkedListRef<T>> {
        let mut curr = Some(self);

        iter::from_fn(move || {
            if let Some(yielded) = curr {
                curr = yielded.right();
                Some(yielded)
            } else {
                None
            }
        })
    }
}

fn main() {
    let alloc = Allocation::new(3);
    let elem_1 = alloc.put(
        0,
        LinkedList {
            value: 1,
            left: None,
            right: None,
        },
    );

    let elem_2 = alloc.put(
        1,
        LinkedList {
            value: 2,
            left: None,
            right: None,
        },
    );

    let elem_3 = alloc.put(
        2,
        LinkedList {
            value: 3,
            left: None,
            right: None,
        },
    );

    elem_2.wrap().insert_after(elem_1.wrap());
    elem_3.wrap().insert_after(elem_2.wrap());

    for (i, elem) in elem_1.wrap().iter_right().enumerate() {
        println!("{i}: {}", elem.value());
    }

    assert!(elem_1.is_alive());
    alloc.take(0);
    assert!(!elem_1.is_alive());
}
