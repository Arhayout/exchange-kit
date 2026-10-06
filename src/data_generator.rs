#[derive(PartialEq)]
pub enum InputLevel {
    One,
    Two,
    Full,
}

pub fn generate_stream<'a>(input_level: InputLevel) -> Vec<&'a [u8]> {
    let mut v = Vec::new();
    // In this case the stream has subscribed to 2 streams, a book stream, and a trade stream.
    v.push(
        "{\
        \"subscription\": [\"PEPEDOGE@BOOK, PEPEDOGE@TRADE\"]\
    }"
        .as_bytes(),
    );
    // A snapshot contains the full current state of the orderbook.
    v.push(
        "{\
        \"event\": \"snapshot\",\
        \"symbol\": \"PEPEDOGE\",\
        \"bids\": [\
            [\"0.0024\",\"24\"],\
            [\"0.0023\",\"11\"],\
            [\"0.0022\",\"22\"],\
            [\"0.0021\",\"87\"],\
            [\"0.0020\",\"24\"],\
            [\"0.0019\",\"12\"],\
            [\"0.0016\",\"32\"],\
            [\"0.0013\",\"7\"],\
            [\"0.0011\",\"6\"],\
            [\"0.0004\",\"120000\"],\
            [\"0.0002\",\"212\"],\
            [\"0.0001\",\"1225\"]\
        ],\
        \"asks\": [\
            [\"0.0026\", \"10\"],\
            [\"0.0027\", \"4\"],\
            [\"0.0028\", \"51\"],\
            [\"0.0029\", \"20\"],\
            [\"0.0030\", \"1\"],\
            [\"0.0033\", \"44\"],\
            [\"0.0039\", \"9\"],\
            [\"0.0055\", \"31\"],\
            [\"0.0072\", \"103\"],\
            [\"0.0120\", \"10\"]\
        ]\
    }"
        .as_bytes(),
    );
    if input_level == InputLevel::One {
        return v;
    }
    // A book update contains the new volumes for levels in the book that have been changed.
    v.push(
        "{\
        \"event\": \"book\",\
        \"symbol\": \"PEPEDOGE\",\
        \"bids\": [\
            [\"0.0024\",\"22\"],\
            [\"0.0023\",\"13\"],\
            [\"0.0022\",\"27\"],\
            [\"0.0021\",\"0\"],\
            [\"0.0020\",\"1230\"]\
        ],\
        \"asks\": [\
            [\"0.0026\", \"13\"],\
            [\"0.0030\", \"0\"]\
        ]\
    }"
        .as_bytes(),
    );
    // A trade always happens at the best available price on one of the sides of the book.
    v.push(
        "{\
        \"event\": \"trade\",\
        \"symbol\": \"PEPEDOGE\",\
        \"price\": \"0.0026\",\
        \"quantity\": \"13\"\
    }"
        .as_bytes(),
    );
    v.push(
        "{\
        \"event\": \"book\",\
        \"symbol\": \"PEPEDOGE\",\
        \"bids\": [
            [\"0.0025\",\"19\"]\
        ],\
        \"asks\": [\
            [\"0.0026\", \"0\"]\
        ]\
    }"
        .as_bytes(),
    );
    if input_level == InputLevel::Two {
        return v;
    }
    v.push(
        "{\
        \"event\": \"trade\",\
        \"symbol\": \"PEPEDOGE\",\
        \"price\": \"0.0024\",\
        \"quantity\": \"17\"\
    }"
        .as_bytes(),
    );
    return v;
}
