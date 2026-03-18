use std::fs;

use chrono::Utc;
use sin_tradedates::Parser;

fn main() {
    let input_path = "examples/klc_td_sh.txt";
    let input = fs::read_to_string(input_path).unwrap();
    let start_index = input.find('"').unwrap() + 1;
    let end_index = input[start_index..].find('"').unwrap() + start_index;

    let s = &input[start_index..end_index];

    let parser = Parser::new(s).unwrap();
    let state = parser.parse().unwrap();
    let dates = state.collect().unwrap();

    let today = Utc::now().date_naive();
    let is_trade_day = dates.contains(&today);
    println!("Is today a trade day? {is_trade_day}");

    println!("Here is the next 5 trade dates:");
    let parser = Parser::new(s).unwrap();
    let state = parser.parse().unwrap();
    let iter = state
        .try_into_iter()
        .unwrap()
        .skip_while(|v| v.as_ref().is_ok_and(|d| *d < today))
        .take(5)
        .flatten();

    for date in iter {
        println!("{date}");
    }
}
