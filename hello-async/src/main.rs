use trpl::{Either, Html};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    trpl::run(async {
        let fut_1 = async {
            let title = page_title(&args[1]).await;
            (&args[1], title)
        };

        let fut_2 = async {
            let title = page_title(&args[2]).await;
            (&args[2], title)
        };

        let (url, maybe_title) = match trpl::race(fut_1, fut_2).await {
            Either::Left(left) => left,
            Either::Right(right) => right,
        };

        println!("{url} returned first");
        match maybe_title {
            Some(title) => println!("Its page title is: '{title}'"),
            None => println!("Its title could not be parsed."),
        }
    })
}

async fn page_title(url: &str) -> Option<String> {
    let response_text = trpl::get(url).await.text().await;

    Html::parse(&response_text)
        .select_first("title")
        .map(|title_element| title_element.inner_html())
}
