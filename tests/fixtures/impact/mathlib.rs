pub fn double(x: i32) -> i32 {
    x * 2
}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubles() {
        let d = double(2);
        assert_eq!(d, 4);
    }
}
