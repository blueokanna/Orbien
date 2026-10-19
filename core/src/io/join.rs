use super::counting::{ByteCounter, CountingStream};
use tokio::io::{copy_bidirectional_with_sizes, AsyncRead, AsyncWrite};

const JOIN_BUF: usize = 256 * 1024;

pub async fn join<A, B>(a: A, b: B) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (a_to_b, b_to_a, err) = join_counted(a, b).await;
    if let Some(e) = err {
        return Err(e);
    }
    Ok((a_to_b, b_to_a))
}

pub async fn join_counted<A, B>(a: A, b: B) -> (u64, u64, Option<std::io::Error>)
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let ca = ByteCounter::new();
    let cb = ByteCounter::new();
    let mut a = CountingStream::new(a, ca.clone());
    let mut b = CountingStream::new(b, cb.clone());

    let err = copy_bidirectional_with_sizes(&mut a, &mut b, JOIN_BUF, JOIN_BUF)
        .await
        .err();

    let a_to_b = ca.read();
    let b_to_a = cb.read();
    (a_to_b, b_to_a, err)
}
