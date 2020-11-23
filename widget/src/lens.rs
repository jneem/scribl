use druid::Lens;

pub struct ReadMap<T, U> {
    map: Box<dyn Fn(&T) -> U>,
}

pub fn read_map<T, U>(f: impl Fn(&T) -> U + 'static) -> ReadMap<T, U> {
    ReadMap { map: Box::new(f) }
}

impl<T, U> Lens<T, U> for ReadMap<T, U> {
    fn with<V, F: FnOnce(&U) -> V>(&self, data: &T, f: F) -> V {
        f(&(self.map)(data))
    }

    fn with_mut<V, F: FnOnce(&mut U) -> V>(&self, data: &mut T, f: F) -> V {
        let mut data = (self.map)(data);
        f(&mut data)
    }
}
