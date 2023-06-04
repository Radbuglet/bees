#[derive(bees::Struct)]
pub struct Example {
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
}

impl ExampleRef {
    pub fn increment(self) {
        self.set_a(self.a() + 1);
        self.set_b(self.b() + 1);
        self.set_c(self.c() + 1);
        self.set_d(self.d() + 1);
        self.set_e(self.e() + 1);
    }
}
