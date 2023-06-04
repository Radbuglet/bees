use bees::{Ptr, Struct};

#[derive(Struct)]
pub struct LinkedList<T> {
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
}

fn main() {
    let elem_1 = Ptr::alloc();
    elem_1.write_new(LinkedList {
        value: 1,
        left: None,
        right: None,
    });

    let elem_2 = Ptr::alloc();
    elem_2.write_new(LinkedList {
        value: 2,
        left: None,
        right: None,
    });

    let elem_3 = Ptr::alloc();
    elem_3.write_new(LinkedList {
        value: 3,
        left: None,
        right: None,
    });

    elem_2.as_wide_ref().insert_after(elem_1.as_wide_ref());
	elem_3.as_wide_ref().insert_after(elem_2.as_wide_ref());
}
