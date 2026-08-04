//! Comparative search-index benchmark.
//!
//! Compares the current index layout / pipeline against the proposed
//! optimizations:
//!
//!   #1/#4  fast-field sort  = `TopDocs::order_by_fast_field(created_at)`
//!                           vs the current score-order + in-memory re-sort.
//!   #3     tokenizer split  = title `ngram(1,3)` + content `ngram(2,3)`
//!                           vs the current single content `ngram(1,3)`.
//!   #2     sync cost        = the cost `search` pays to drain a pending
//!                           `search_outbox` versus searching an idle index.
//!
//! This benchmark is `#[ignore]`d so the normal suite stays fast. Run it in
//! release mode for meaningful timings:
//!
//! ```text
//! cargo test --release --test search_bench -- --ignored --nocapture
//! ```
//!
//! Results are printed as `key=value` lines so they can be collected into a
//! report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, FAST, STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, Order, ReloadPolicy, TantivyDocument};

const REPEATS: u32 = 40;

struct Fields {
    item_id: Field,
    created_at_ms: Field,
    title: Field,
    content: Field,
}

fn fields_for(schema: &Schema) -> Fields {
    Fields {
        item_id: schema.get_field("item_id").unwrap(),
        created_at_ms: schema.get_field("created_at_ms").unwrap(),
        title: schema.get_field("title").unwrap(),
        content: schema.get_field("content").unwrap(),
    }
}

fn register(index: &mut Index) {
    index.tokenizers().register(
        "content_a",
        TextAnalyzer::builder(NgramTokenizer::all_ngrams(1, 3).unwrap())
            .filter(LowerCaser)
            .build(),
    );
    index.tokenizers().register(
        "content_b",
        TextAnalyzer::builder(NgramTokenizer::all_ngrams(2, 3).unwrap())
            .filter(LowerCaser)
            .build(),
    );
    index.tokenizers().register(
        "title_b",
        TextAnalyzer::builder(NgramTokenizer::all_ngrams(1, 3).unwrap())
            .filter(LowerCaser)
            .build(),
    );
}

// Variant A mirrors the current production layout: a single content field
// tokenized with 1-3 char n-grams.
// Variant B splits the (usually short) title, which keeps the full 1-3 gram
// range for single-character search, from the (usually long) full text, which
// only builds 2-3 gram terms to shrink the index.
fn open_index(root: &PathBuf, variant: &str) -> (Index, Fields) {
    std::fs::create_dir_all(root).unwrap();
    let mut builder = Schema::builder();
    builder.add_text_field("item_id", STRING | FAST | STORED);
    builder.add_text_field("title", STRING | FAST | STORED);
    builder.add_i64_field("created_at_ms", FAST | STORED);
    let content_indexing = TextFieldIndexing::default()
        .set_tokenizer(if variant == "A" {
            "content_a"
        } else {
            "content_b"
        })
        .set_index_option(IndexRecordOption::WithFreqs);
    builder.add_text_field(
        "content",
        TextOptions::default().set_indexing_options(content_indexing),
    );
    let schema = builder.build();
    let fields = fields_for(&schema);
    let mut index = Index::open_or_create(MmapDirectory::open(root).unwrap(), schema).unwrap();
    register(&mut index);
    (index, fields)
}

struct BenchDoc {
    item_id: String,
    title: String,
    content: String,
    created_at_ms: i64,
}

fn add_docs(index: &mut Index, fields: &Fields, corpus: &[BenchDoc]) {
    let mut writer = index
        .writer_with_num_threads::<TantivyDocument>(4, 60_000_000)
        .unwrap();
    for d in corpus {
        let mut doc = TantivyDocument::new();
        doc.add_text(fields.item_id, &d.item_id);
        doc.add_text(fields.title, &d.title);
        doc.add_i64(fields.created_at_ms, d.created_at_ms);
        doc.add_text(fields.content, &d.content);
        writer.add_document(doc).unwrap();
    }
    writer.commit().unwrap();
    writer.wait_merging_threads().unwrap();
}

fn open_reader(index: &Index) -> IndexReader {
    index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .unwrap()
}

fn term_query(field: Field, term: &str) -> Box<dyn tantivy::query::Query> {
    Box::new(TermQuery::new(
        tantivy::Term::from_field_text(field, term),
        IndexRecordOption::WithFreqs,
    ))
}

// Mirrors `SearchQuery::required_ngrams`: terms of 1-3 chars stay intact,
// longer terms become overlapping 3-gram windows.
fn to_ngrams(term: &str) -> Vec<String> {
    let chars: Vec<char> = term.chars().collect();
    if chars.len() <= 3 {
        return vec![term.to_owned()];
    }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

fn build_query(field: Field, terms: &[String]) -> Box<dyn tantivy::query::Query> {
    let mut grams = Vec::new();
    for t in terms {
        grams.extend(to_ngrams(t));
    }
    let qs: Vec<Box<dyn tantivy::query::Query>> =
        grams.into_iter().map(|g| term_query(field, &g)).collect();
    Box::new(BooleanQuery::intersection(qs))
}

// Variant B routes each gram to the field that still contains it: 1-char grams
// only exist in the title (1-3 gram) field, 2-3 char grams exist in content.
fn build_query_split(fields: &Fields, terms: &[String]) -> Box<dyn tantivy::query::Query> {
    let mut title_grams = Vec::new();
    let mut content_grams = Vec::new();
    for t in terms {
        for g in to_ngrams(t) {
            if g.chars().count() == 1 {
                title_grams.push(g);
            } else {
                content_grams.push(g);
            }
        }
    }
    let qs: Vec<Box<dyn tantivy::query::Query>> = title_grams
        .into_iter()
        .map(|g| term_query(fields.title, &g))
        .chain(
            content_grams
                .into_iter()
                .map(|g| term_query(fields.content, &g)),
        )
        .collect();
    Box::new(BooleanQuery::intersection(qs))
}

fn search_hits(reader: &IndexReader, q: &dyn tantivy::query::Query, limit: usize) -> usize {
    let searcher = reader.searcher();
    searcher
        .search(q, &TopDocs::with_limit(limit).order_by_score())
        .unwrap()
        .len()
}

// Baseline pipeline tail: relevance query -> candidate IDs -> read sort key ->
// in-memory descending sort (what `apply_sort_rules` does for CreatedAt today).
fn pipeline_score_sorted(
    reader: &IndexReader,
    fields: &Fields,
    terms: &[String],
    limit: usize,
) -> Vec<i64> {
    let searcher = reader.searcher();
    let hits = searcher
        .search(
            &build_query(fields.content, terms),
            &TopDocs::with_limit(limit).order_by_score(),
        )
        .unwrap();
    let mut keys: Vec<i64> = hits
        .iter()
        .map(|(_s, addr)| {
            searcher
                .doc::<TantivyDocument>(*addr)
                .unwrap()
                .get_first(fields.created_at_ms)
                .unwrap()
                .as_i64()
                .unwrap()
        })
        .collect();
    keys.sort_unstable_by(|a, b| b.cmp(a));
    keys
}

// Proposed fast path: FAST field ordering inside Tantivy, no re-sort needed.
fn pipeline_fast_sorted(
    reader: &IndexReader,
    fields: &Fields,
    terms: &[String],
    limit: usize,
) -> Vec<i64> {
    let searcher = reader.searcher();
    let hits = searcher
        .search(
            &build_query(fields.content, terms),
            &TopDocs::with_limit(limit).order_by_fast_field::<i64>("created_at_ms", Order::Desc),
        )
        .unwrap();
    hits.into_iter()
        .filter_map(|(k, _a)| k)
        .collect()
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    fn walk(p: &std::path::Path, t: &mut u64) {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            let meta = e.metadata().unwrap();
            if meta.is_dir() {
                walk(&e.path(), t);
            } else {
                *t += meta.len();
            }
        }
    }
    walk(path, &mut total);
    total
}

fn summary(samples: &[Duration]) -> (f64, f64) {
    let mut v: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1_000.0).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = v[v.len() / 2];
    let p95 = v[((v.len() as f64) * 0.95).floor() as usize];
    (p50, p95)
}

fn timed<R>(mut f: impl FnMut() -> R, n: u32) -> (Vec<Duration>, R) {
    let mut samples = Vec::with_capacity(n as usize);
    let mut last = None;
    for _ in 0..n {
        let t = Instant::now();
        last = Some(f());
        samples.push(t.elapsed());
    }
    (samples, last.unwrap())
}

fn build_corpus(count: usize) -> Vec<BenchDoc> {
    let zh_words = [
        "人脸",
        "身份",
        "账号",
        "密码",
        "验证码",
        "会议",
        "地址",
        "订单",
        "发票",
        "合同",
        "截图",
        "设计稿",
        "笔记",
        "计划",
        "报错",
        "日志",
        "配置",
        "部署",
        "发布",
        "备份",
    ];
    let zh_tail = [
        "脏乱",
        "规范",
        "紧急",
        "待办",
        "已完成",
        "测试中",
        "已归档",
        "需复核",
        "暂缓",
        "正常",
    ];
    let en = [
        "clipboard",
        "commit",
        "search",
        "index",
        "tokenize",
        "pipeline",
        "collector",
        "cargo",
        "release",
        "build",
    ];
    let mut rng = hash_rng(42);
    (0..count)
        .map(|i| {
            let c = rng();
            let w = zh_words[(c % zh_words.len() as u64) as usize];
            let t = zh_tail[((c >> 4) % zh_tail.len() as u64) as usize];
            let e = en[((c >> 8) % en.len() as u64) as usize];
            let sentence = format!(
                "{w} {t} {e} 在项目中有标签 {} 其它文本 {} hash {}",
                c % 7,
                c % 11,
                c
            );
            BenchDoc {
                item_id: format!("item-{i}"),
                title: format!("{w} {t}"),
                content: sentence,
                created_at_ms: 1_700_000_000_000 + i as i64,
            }
        })
        .collect()
}

fn hash_rng(mut s: u64) -> impl FnMut() -> u64 {
    move || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        s >> 33
    }
}

fn measure() {
    for count in [20_000usize, 80_000usize] {
        let corpus = build_corpus(count);
        let root = std::env::temp_dir().join(format!(
            "clipboard-search-bench-{count}-{}",
            std::process::id()
        ));

        let (mut ia, fa) = open_index(&root.join("A"), "A");
        let (mut ib, fb) = open_index(&root.join("B"), "B");
        let (build_a, _) = timed(|| add_docs(&mut ia, &fa, &corpus), 1);
        let (build_b, _) = timed(|| add_docs(&mut ib, &fb, &corpus), 1);
        drop(ia);
        drop(ib);

        println!("\n== corpus={count} ==");
        println!("key=build_A_ms:{} corpus={count}", summary(&build_a).0);
        println!("key=build_B_ms:{} corpus={count}", summary(&build_b).0);
        println!(
            "key=size_A_bytes:{} corpus={count}",
            dir_size(&root.join("A"))
        );
        println!(
            "key=size_B_bytes:{} corpus={count}",
            dir_size(&root.join("B"))
        );

        let (ra, fa) = open_index(&root.join("A"), "A");
        let (rb, fb) = open_index(&root.join("B"), "B");
        let reader_a = open_reader(&ra);
        let reader_b = open_reader(&rb);

        let queries: Vec<(&str, Vec<String>)> = vec![
            ("single_char_cn", vec!["脏".into()]),
            ("bigram_cn", vec!["验证".into(), "密码".into()]),
            ("two_terms_cn", vec!["人脸".into(), "发布".into()]),
            ("en_term", vec!["deploy".into()]),
            (
                "mixed_long",
                vec!["人脸".into(), "deploy".into(), "备份".into()],
            ),
        ];

        for (name, terms) in &queries {
            let qa = build_query(fa.content, terms);
            let qb = build_query_split(&fb, terms);
            let (sa, _) = timed(|| search_hits(&reader_a, &qa, 500), REPEATS);
            let (sb, _) = timed(|| search_hits(&reader_b, &qb, 500), REPEATS);
            let (p50a, p95a) = summary(&sa);
            let (p50b, p95b) = summary(&sb);
            println!(
                "key=lat_A_ms_p50:{} p95:{} query:{name} corpus={count}",
                p50a, p95a
            );
            println!(
                "key=lat_B_ms_p50:{} p95:{} query:{name} corpus={count}",
                p50b, p95b
            );
        }

        let (pa, _) = timed(
            || pipeline_score_sorted(&reader_a, &fa, &queries[4].1, 500),
            REPEATS,
        );
        let (pb, _) = timed(
            || pipeline_fast_sorted(&reader_a, &fa, &queries[4].1, 500),
            REPEATS,
        );
        println!(
            "key=pipeline_score_ms_p50:{} p95:{} query:{} corpus={count}",
            summary(&pa).0,
            summary(&pa).1,
            queries[4].0
        );
        println!(
            "key=pipeline_fast_ms_p50:{} p95:{} query:{} corpus={count}",
            summary(&pb).0,
            summary(&pb).1,
            queries[4].0
        );

        drop(reader_a);
        drop(reader_b);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

#[test]
#[ignore = "performance benchmark; run explicitly in release mode"]
fn search_benchmark() {
    measure();
}

// #2: quantify what a pending search_outbox costs the search hot path.
// Today `search_clipboard_items` blocks on `sync_until_idle` whenever events
// are pending; a background worker (#2) would move that cost off the query.
#[test]
#[ignore = "performance benchmark; run explicitly in release mode"]
fn sync_cost_benchmark() {
    for pending in [0usize, 100, 1_000, 10_000] {
        let samples = timed_drain(pending, REPEATS);
        let (p50, p95) = summary(&samples);
        println!(
            "key=sync_ms_p50:{} p95:{} pending={} corpus=pending",
            p50, p95, pending
        );
    }
}

// Seeds `pending` fresh outbox events and times the drain that the search hot
// path performs (`sync_until_idle`). Each sample re-seeds so every iteration
// measures the same workload instead of measuring an already-idle index.
fn timed_drain(pending: usize, n: u32) -> Vec<Duration> {
    use clipboard_desktop_lib::domain::{ClipboardItem, ClipboardKind};
    use clipboard_desktop_lib::search::{SearchIndex, SearchSynchronizer};
    use clipboard_desktop_lib::storage::{ClipboardRepository, Database, SearchRepository};

    fn item(id: &str, created_at_ms: i64) -> ClipboardItem {
        ClipboardItem {
            id: id.to_owned(),
            kind: ClipboardKind::Text,
            title: "人脸 脏乱".to_owned(),
            text_content: Some("人脸 脏乱 在项目中有标签内容".to_owned()),
            html_content: None,
            resource_path: None,
            preview_path: None,
            content_hash: format!("hash-{id}"),
            source_app: Some("bench".to_owned()),
            size_bytes: 32,
            created_at_ms,
            last_used_at_ms: None,
            is_favorite: false,
            icon_path: None,
            metadata_json: None,
        }
    }

    let mut samples = Vec::with_capacity(n as usize);
    for round in 0..n {
        let db = Database::open_in_memory().unwrap();
        let index = SearchIndex::in_memory().unwrap();
        let base = (round as u64) * 1_000_000;
        for i in 0..pending {
            db.save_item(&item(
                &format!("pending-{round}-{i}"),
                (base + i as u64) as i64,
            ))
            .unwrap();
        }
        if pending == 0 {
            db.save_item(&item("seed", 1_700_000_000_000)).unwrap();
            db.acknowledge_search_outbox(
                db.read_search_outbox(1000)
                    .unwrap()
                    .last()
                    .unwrap()
                    .sequence,
            )
            .unwrap();
        }
        let t = Instant::now();
        SearchSynchronizer::default()
            .sync_until_idle(&db, &index)
            .unwrap();
        samples.push(t.elapsed());
    }
    samples
}

#[allow(dead_code)]
fn _unused(_: &HashMap<String, (f64, f64)>) {}
