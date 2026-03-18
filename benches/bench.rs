#![feature(test)]

extern crate test;

use std::{fs, mem};
use test::Bencher;

use sin_tradedates::Parser;

#[bench]
fn bench_collect(b: &mut Bencher) {
    let input = load_data();

    b.iter(|| {
        let parser = Parser::new(&input).unwrap();
        let state = parser.parse().unwrap();
        state.collect().unwrap()
    });
}

#[bench]
fn bench_iter(b: &mut Bencher) {
    let input = load_data();

    b.iter(|| {
        let parser = Parser::new(&input).unwrap();
        let state = parser.parse().unwrap();
        state.try_into_iter().unwrap().for_each(mem::drop);
    });
}

fn load_data() -> String {
    let input_path = "examples/klc_td_sh.txt";
    let input = fs::read_to_string(input_path).unwrap();
    let start_index = input.find('"').unwrap() + 1;
    let end_index = input[start_index..].find('"').unwrap() + start_index;

    input[start_index..end_index].to_string()
}
