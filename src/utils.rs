use std::fmt::Debug;
use tokio::sync::mpsc::Receiver;

use crate::errors::InvalidInput;

// pub async fn read_rx<T: Debug>(name: &str, rx: Receiver<T>) -> Result<(), Box<dyn Error>> {
//     println!("Listening for {}", name);
//     let mut i = 0;
//     loop {
//         let thing = rx.recv().await?;
//         i += 1;
//         println!("#{} {}: {:#?}", i, name, thing);
//     }
// }

/// e.g. convert
///
/// [[ 0,  1,  2,  3,  4],
///  [10, 11, 12, 13, 14],
///  [20, 21, 22, 23, 24],
///  [30, 31, 32, 33, 34],
///  [40, 41, 42, 43, 44]]
///
/// ->
///
/// [[ 0, 10, 20, 30, 40],
///  [ 1, 11, 21, 31, 41],
///  [ 2, 12, 22, 32, 42],
///  [ 3, 13, 23, 33, 43],
///  [ 4, 14, 24, 34, 44]]
pub fn transpose_vecvec<U, T>(orig: &[U]) -> Result<Vec<Vec<T>>, InvalidInput>
where
    U: AsRef<[T]>,
    T: Clone,
{
    let outer_len = orig.len();
    if outer_len == 0 {
        return Ok(Vec::new());
    }
    let inner_len = orig[0].as_ref().len();

    for (i, inner) in orig[1..].iter().enumerate() {
        if inner.as_ref().len() != inner_len {
            return Err(InvalidInput::Message(format!(
                "transpose_vecvec: row 0 has len {}, row {} has len {}",
                inner_len,
                i + 1,
                inner.as_ref().len()
            )));
        }
    }

    let mut result = Vec::<Vec<T>>::with_capacity(inner_len);
    for _ in 0..inner_len {
        result.push(Vec::<T>::with_capacity(outer_len));
    }

    for inner in orig {
        let slice = inner.as_ref();
        for (j, el) in slice.iter().enumerate() {
            result[j].push((*el).clone());
        }
    }

    Ok(result)
}

/// Pop last n elements from Vec
/// See https://stackoverflow.com/a/28952552/4228052
pub fn pop_n<T>(v: &mut Vec<T>, n: usize) -> Vec<T> {
    let final_length = v.len().saturating_sub(n);
    v.split_off(final_length)
}
