use chrono::NaiveDateTime;

#[derive(Serialize, Deserialize, Debug)]
pub struct EncodableKeyword {
    pub id: String,
    pub keyword: String,
    pub created_at: NaiveDateTime,
    pub crates_cnt: i32,
}
