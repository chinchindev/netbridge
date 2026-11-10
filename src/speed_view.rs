use crate::speed::ResultRow;
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeedUnit {
    #[default]
    Mbps,
    #[serde(rename = "MB/s")]
    Megabytes,
    #[serde(rename = "MiB/s")]
    Mebibytes,
    Kbps,
    Gbps,
}
impl SpeedUnit {
    pub const ALL: [Self; 5] = [
        Self::Mbps,
        Self::Megabytes,
        Self::Mebibytes,
        Self::Kbps,
        Self::Gbps,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Mbps => "Mbps",
            Self::Megabytes => "MB/s",
            Self::Mebibytes => "MiB/s",
            Self::Kbps => "Kbps",
            Self::Gbps => "Gbps",
        }
    }
    pub fn convert(self, mbps: f64) -> Option<f64> {
        if !mbps.is_finite() || mbps < 0.0 {
            return None;
        }
        let value = match self {
            Self::Mbps => mbps,
            Self::Megabytes => mbps / 8.0,
            Self::Mebibytes => mbps * 1_000_000.0 / 8.0 / 1_048_576.0,
            Self::Kbps => mbps * 1000.0,
            Self::Gbps => mbps / 1000.0,
        };
        value.is_finite().then_some(value)
    }
    pub fn format(self, value: Option<f64>) -> String {
        value
            .and_then(|value| self.convert(value))
            .map(|value| format!("{value:.2} {}", self.label()))
            .unwrap_or_else(|| "Chưa đo được".into())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Excellent,
    Good,
    Fair,
    Poor,
    Incomplete,
    Unavailable,
}
impl Grade {
    pub fn label(self) -> &'static str {
        match self {
            Self::Excellent => "Rất tốt",
            Self::Good => "Tốt",
            Self::Fair => "Trung bình",
            Self::Poor => "Yếu",
            Self::Incomplete => "Chưa đủ dữ liệu",
            Self::Unavailable => "Chưa đo được",
        }
    }
}
#[derive(Debug)]
pub struct Assessment {
    pub grade: Grade,
    pub explanation: String,
}
pub fn assess(row: &ResultRow) -> Assessment {
    let values = [row.download_mbps, row.upload_mbps, row.latency_ms];
    if values.iter().all(Option::is_none) {
        return Assessment {
            grade: Grade::Unavailable,
            explanation: "Chưa có số liệu để đánh giá đường mạng.".into(),
        };
    }
    if !row.errors.is_empty()
        || values
            .iter()
            .any(|v| v.is_none_or(|n| !n.is_finite() || n < 0.0))
    {
        return Assessment {
            grade: Grade::Incomplete,
            explanation: "Thiếu chỉ số hoặc phép đo có lỗi; cần đo lại trước khi kết luận.".into(),
        };
    }
    let bandwidth = |value: f64, excellent: f64, good: f64, fair: f64| {
        if value >= excellent {
            3
        } else if value >= good {
            2
        } else if value >= fair {
            1
        } else {
            0
        }
    };
    let latency = row.latency_ms.unwrap();
    let scores = [
        (
            "download",
            bandwidth(row.download_mbps.unwrap(), 100.0, 25.0, 10.0),
        ),
        (
            "upload",
            bandwidth(row.upload_mbps.unwrap(), 20.0, 5.0, 1.0),
        ),
        (
            "độ trễ HTTP",
            if latency <= 60.0 {
                3
            } else if latency <= 120.0 {
                2
            } else if latency <= 200.0 {
                1
            } else {
                0
            },
        ),
    ];
    let lowest = scores.iter().map(|(_, score)| *score).min().unwrap();
    let grade = match lowest {
        3 => Grade::Excellent,
        2 => Grade::Good,
        1 => Grade::Fair,
        _ => Grade::Poor,
    };
    let explanation = if lowest == 3 {
        "Cả download, upload và độ trễ HTTP đều đạt ngưỡng rất tốt.".into()
    } else {
        format!(
            "Chỉ số giới hạn: {}.",
            scores
                .iter()
                .filter(|(_, score)| *score == lowest)
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Assessment { grade, explanation }
}
pub const CRITERIA:&str="Ngưỡng tham khảo của NetBridge (download / upload / HTTP latency):\n\nRất tốt: ≥100 Mbps / ≥20 Mbps / ≤60 ms\nTốt: ≥25 Mbps / ≥5 Mbps / ≤120 ms\nTrung bình: ≥10 Mbps / ≥1 Mbps / ≤200 ms\nYếu: có chỉ số thấp hơn các ngưỡng trên.\n\nMức chung là mức thấp nhất của ba chỉ số. Thiếu chỉ số hoặc có lỗi: chưa đủ dữ liệu.\nĐây không phải tiêu chuẩn của nhà mạng. Không đo jitter, mất gói hoặc độ ổn định dài hạn.\n\n1 MB/s = 8 Mbps; 1 MiB/s = 8,388608 Mbps. Đổi đơn vị không chạy phép đo mới.";

pub fn render_result(row: &ResultRow, unit: SpeedUnit) -> String {
    let assessment = assess(row);
    let latency = row
        .latency_ms
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| format!("{v:.2} ms"))
        .unwrap_or_else(|| "Chưa đo được".into());
    let errors = if row.errors.is_empty() {
        String::new()
    } else {
        format!("\r\nLỗi đo: {}", row.errors.join(" · "))
    };
    format!("{}  ·  IPv4 {}\r\nDownload: {}    |    Upload: {}    |    HTTP: {}\r\nĐánh giá: {} (tham khảo) · {}{}\r\n",
        row.adapter,row.source_ip,unit.format(row.download_mbps),unit.format(row.upload_mbps),latency,
        assessment.grade.label(),assessment.explanation,errors)
}
#[derive(Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub unit: SpeedUnit,
}
impl Preferences {
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            std::fs::copy(path, path.with_extension("json.bak"))?;
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )
    }
}
