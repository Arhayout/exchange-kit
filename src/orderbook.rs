use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, PartialEq)]
pub struct Bid {
    price: Decimal,
    quantity: u64,
}

#[derive(Debug, PartialEq)]
pub struct Ask {
    price: Decimal,
    quantity: u64,
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    event: Option<String>,
    bids: Option<Vec<[String; 2]>>,
    asks: Option<Vec<[String; 2]>>,
    price: Option<String>,
    quantity: Option<String>,
}

struct OrderBook {
    /// Ascending by price. Best bid = last entry.
    bids: BTreeMap<Decimal, u64>,
    /// Ascending by price. Best ask = first entry.
    asks: BTreeMap<Decimal, u64>,
}

impl OrderBook {
    fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    fn apply_levels(side: &mut BTreeMap<Decimal, u64>, levels: &[[String; 2]]) {
        for level in levels {
            // Skip malformed levels rather than panicking on untrusted stream data.
            let Ok(price) = Decimal::from_str(&level[0]) else {
                continue;
            };
            let Ok(quantity) = level[1].parse::<u64>() else {
                continue;
            };
            if quantity == 0 {
                side.remove(&price);
            } else {
                side.insert(price, quantity);
            }
        }
    }

    fn replace_side(side: &mut BTreeMap<Decimal, u64>, levels: &[[String; 2]]) {
        side.clear();
        Self::apply_levels(side, levels);
    }

    fn apply_snapshot(&mut self, bids: &[[String; 2]], asks: &[[String; 2]]) {
        Self::replace_side(&mut self.bids, bids);
        Self::replace_side(&mut self.asks, asks);
    }

    fn apply_book_update(&mut self, bids: &[[String; 2]], asks: &[[String; 2]]) {
        Self::apply_levels(&mut self.bids, bids);
        Self::apply_levels(&mut self.asks, asks);
    }

    /// Apply a trade print for this stream dialect.
    ///
    /// Trades may print away from the visible best bid (e.g. 0.0024 while
    /// 0.0025 still rests) with no trailing book delta — so we retire strictly
    /// better levels on the hit side, then subtract quantity at the print price.
    fn apply_trade(&mut self, price: Decimal, quantity: u64) {
        if self.bids.contains_key(&price) {
            self.bids.retain(|&p, _| p <= price);
            Self::take_quantity(&mut self.bids, price, quantity);
        } else if self.asks.contains_key(&price) {
            self.asks.retain(|&p, _| p >= price);
            Self::take_quantity(&mut self.asks, price, quantity);
        }
    }

    fn take_quantity(side: &mut BTreeMap<Decimal, u64>, price: Decimal, quantity: u64) {
        if let Some(qty) = side.get_mut(&price) {
            *qty = qty.saturating_sub(quantity);
            if *qty == 0 {
                side.remove(&price);
            }
        }
    }

    fn best_bids(&self, n: usize) -> Vec<Bid> {
        self.bids
            .iter()
            .rev()
            .take(n)
            .map(|(&price, &quantity)| Bid { price, quantity })
            .collect()
    }

    fn best_asks(&self, n: usize) -> Vec<Ask> {
        self.asks
            .iter()
            .take(n)
            .map(|(&price, &quantity)| Ask { price, quantity })
            .collect()
    }
}

/// Replay a book+trade stream and return the top 5 bids / asks.
///
/// Accepts any iterator of JSON message bytes (`Vec<&[u8]>` works unchanged).
/// Malformed JSON messages and unparsable levels/trades are skipped so a bad
/// frame cannot abort the process.
pub fn get_best_bids_and_asks_from_stream<'a, I>(stream: I) -> (Vec<Bid>, Vec<Ask>)
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut book = OrderBook::new();

    for raw in stream {
        let msg: StreamMessage = match serde_json::from_slice(raw) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match msg.event.as_deref() {
            Some("snapshot") => {
                book.apply_snapshot(
                    msg.bids.as_deref().unwrap_or_default(),
                    msg.asks.as_deref().unwrap_or_default(),
                );
            }
            Some("book") => {
                book.apply_book_update(
                    msg.bids.as_deref().unwrap_or_default(),
                    msg.asks.as_deref().unwrap_or_default(),
                );
            }
            Some("trade") => {
                let (Some(price_s), Some(qty_s)) = (msg.price.as_deref(), msg.quantity.as_deref())
                else {
                    continue;
                };
                let Ok(price) = Decimal::from_str(price_s) else {
                    continue;
                };
                let Ok(quantity) = qty_s.parse::<u64>() else {
                    continue;
                };
                book.apply_trade(price, quantity);
            }
            _ => {}
        }
    }

    (book.best_bids(5), book.best_asks(5))
}

#[cfg(test)]
mod tests {
    use crate::{
        data_generator::{generate_stream, InputLevel},
        orderbook::{get_best_bids_and_asks_from_stream, Ask, Bid},
    };
    use rust_decimal_macros::dec;

    #[test]
    fn test_data_snapshot() {
        let stream = generate_stream(InputLevel::One);

        let (best_bids, best_asks) = get_best_bids_and_asks_from_stream(stream);

        assert_eq!(
            best_bids,
            vec![
                Bid {
                    price: dec!(0.0024),
                    quantity: 24
                },
                Bid {
                    price: dec!(0.0023),
                    quantity: 11
                },
                Bid {
                    price: dec!(0.0022),
                    quantity: 22
                },
                Bid {
                    price: dec!(0.0021),
                    quantity: 87
                },
                Bid {
                    price: dec!(0.0020),
                    quantity: 24
                }
            ]
        );

        assert_eq!(
            best_asks,
            vec![
                Ask {
                    price: dec!(0.0026),
                    quantity: 10
                },
                Ask {
                    price: dec!(0.0027),
                    quantity: 4
                },
                Ask {
                    price: dec!(0.0028),
                    quantity: 51
                },
                Ask {
                    price: dec!(0.0029),
                    quantity: 20
                },
                Ask {
                    price: dec!(0.0030),
                    quantity: 1
                }
            ]
        );
    }

    #[test]
    fn test_data_without_final_trade() {
        let stream = generate_stream(InputLevel::Two);

        let (best_bids, best_asks) = get_best_bids_and_asks_from_stream(stream);

        assert_eq!(
            best_bids,
            vec![
                Bid {
                    price: dec!(0.0025),
                    quantity: 19
                },
                Bid {
                    price: dec!(0.0024),
                    quantity: 22
                },
                Bid {
                    price: dec!(0.0023),
                    quantity: 13
                },
                Bid {
                    price: dec!(0.0022),
                    quantity: 27
                },
                Bid {
                    price: dec!(0.0020),
                    quantity: 1230
                }
            ]
        );

        assert_eq!(
            best_asks,
            vec![
                Ask {
                    price: dec!(0.0027),
                    quantity: 4
                },
                Ask {
                    price: dec!(0.0028),
                    quantity: 51
                },
                Ask {
                    price: dec!(0.0029),
                    quantity: 20
                },
                Ask {
                    price: dec!(0.0033),
                    quantity: 44
                },
                Ask {
                    price: dec!(0.0039),
                    quantity: 9
                }
            ]
        );
    }

    #[test]
    fn test_data_with_final_trade() {
        let stream = generate_stream(InputLevel::Full);

        let (best_bids, best_asks) = get_best_bids_and_asks_from_stream(stream);

        assert_eq!(
            best_bids,
            vec![
                Bid {
                    price: dec!(0.0024),
                    quantity: 5
                },
                Bid {
                    price: dec!(0.0023),
                    quantity: 13
                },
                Bid {
                    price: dec!(0.0022),
                    quantity: 27
                },
                Bid {
                    price: dec!(0.0020),
                    quantity: 1230
                },
                Bid {
                    price: dec!(0.0019),
                    quantity: 12
                }
            ]
        );

        assert_eq!(
            best_asks,
            vec![
                Ask {
                    price: dec!(0.0027),
                    quantity: 4
                },
                Ask {
                    price: dec!(0.0028),
                    quantity: 51
                },
                Ask {
                    price: dec!(0.0029),
                    quantity: 20
                },
                Ask {
                    price: dec!(0.0033),
                    quantity: 44
                },
                Ask {
                    price: dec!(0.0039),
                    quantity: 9
                }
            ]
        );
    }
}
