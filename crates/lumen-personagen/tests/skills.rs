use lumen_debate::pack::ShowPack;
use lumen_personagen::{
    DraftSource, Options, SourceError,
    skill::{generate_skills, sample_lines, skill_path, slug},
};
use std::{cell::RefCell, path::Path};

const PACK: &str = include_str!("../../../packs/personas/history-clash.toml");

fn sheet(lines: &[&str]) -> serde_json::Value {
    let long =
        "Sakin ama inatçı; her cevaba önce karşı tarafın en güçlü argümanını özetleyerek başlar.";
    serde_json::json!({
        "summary": long, "identity": long, "voice": long, "humor": long,
        "objections": long, "audience": long,
        "signature_phrases": ["Bir saniye", "Kayıtlara geçsin", "Şöyle düşünelim", "Tamam, kabul"],
        "argument_moves": ["Karşı argümanı güçlendirip sonra çürütür.", "Somut bir örneğe iner.", "Sınırı netleştirir."],
        "talking_points": (0..6).map(|i| format!("Bizim okumamıza göre bilinen nokta numara {i}.")).collect::<Vec<_>>(),
        "punchlines": ["Okul açan kazanır, kapı kapatan kaybeder.", "Biz soru sorduk, onlar cevap kaçırdı.", "Üç kelime: okul, yol, hukuk.", "Şartlar mazeret değil, açıklamadır.", "Halk için mi, halka rağmen mi?", "Hatırlayın, karar kimindi?"],
        "values": ["Vatan: bağımsızlığı her cümlenin merkezine koyar.", "Adalet: kimin bedel ödediğini sorar.", "Gelecek: çocukların okulunu örnek verir."],
        "spin_moves": ["Soruyu kendi sahasına çeker ve çerçeveyi değiştirir.", "İspat yükünü rakibe geri atar.", "Rakibin cümlesini tersine çevirir."],
        "boundaries": ["Kişisel hakarete girmez.", "Bilmediği konuda tahmin yürütmez."],
        "sample_lines": lines,
    })
}

struct Fixed(RefCell<Vec<String>>);
impl DraftSource for Fixed {
    fn draft(&self, prompt: &str, schema: &str) -> Result<String, SourceError> {
        assert!(schema.contains("sample_lines"));
        assert!(prompt.contains("topic 1:"));
        assert!(prompt.contains("<playbook>\nPLAYBOOK"));
        assert!(prompt.contains("EDITOR REVIEW") && prompt.contains("- Replikler fazla uzlaşmacı"));
        Ok(self.0.borrow_mut().remove(0))
    }
    fn describe(&self) -> String {
        "fixed".into()
    }
}

#[test]
fn skill_sheets_render_parse_and_retry() {
    let pack = ShowPack::parse(PACK).unwrap();
    let a: Vec<String> = (0..9)
        .map(|i| format!("Ada'nın örnek repliği numara {i}, kısa ve net."))
        .collect();
    let b: Vec<String> = (0..9)
        .map(|i| format!("Deniz'in örnek repliği numara {i}, iddialı."))
        .collect();
    let a_ref: Vec<&str> = a.iter().map(String::as_str).collect();
    let b_ref: Vec<&str> = b.iter().map(String::as_str).collect();
    let bad = serde_json::json!({"first": sheet(&a_ref[..3]), "second": sheet(&b_ref)}).to_string();
    let good = serde_json::json!({"first": sheet(&a_ref), "second": sheet(&b_ref)}).to_string();
    let source = Fixed(RefCell::new(vec![bad, good]));
    let options = Options {
        attempts: 3,
        origin: "pack.toml",
        progress: &|_, _| {},
    };
    let (sheets, attempts) = generate_skills(
        &pack,
        "PLAYBOOK",
        &["Replikler fazla uzlaşmacı".into()],
        &source,
        &options,
    )
    .unwrap();
    assert_eq!(attempts, 2);
    assert!(sheets[0].starts_with("---\nname: ada\n"));
    assert!(sheets[0].contains("## İmza ifadeler"));
    assert!(sheets[0].contains("## Bilgi birikimi"));
    assert!(sheets[0].contains("## Spin taktikleri"));
    assert!(sheets[0].contains("## Vuruş cümleleri"));
    assert!(sheets[0].contains("## Değer çapaları"));
    assert!(sheets[0].contains("Kaynak, istatistik, tarih veya alıntı uydurmazsın"));
    assert_eq!(sample_lines(&sheets[0]), a);
    assert_eq!(sample_lines(&sheets[1]), b);
}

#[test]
fn slugs_and_paths_follow_convention() {
    assert_eq!(slug("Işıl Çağrı"), "isil-cagri");
    assert_eq!(slug("  Ömer-Şükrü "), "omer-sukru");
    assert_eq!(
        skill_path(Path::new("packs/personas/history-clash.toml"), "Ada"),
        Path::new("packs/skills/history-clash/ada.md")
    );
}
