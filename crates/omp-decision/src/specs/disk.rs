use super::{Priority, Specification, megabytes, size_suffix};
use crate::{
    model::{DecisionData, DecisionInfo, Subject},
    reason::{RejectionReason as R, Verdict},
};

/// Is there room to import the release? Runs last because it touches the disk.
pub struct FreeSpace;

impl Specification for FreeSpace {
    fn name(&self) -> &'static str {
        "FreeSpace"
    }

    fn priority(&self) -> Priority {
        Priority::Disk
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let config = env.config();
        if config.skip_free_space_check_when_importing {
            return Verdict::Accept;
        }
        let Some(free) = env.free_space(&s.movie.path) else {
            return Verdict::Accept;
        };
        let minimum = megabytes(config.minimum_free_space_when_importing as f64);
        let remaining = free as i64 - s.release.size as i64;
        if remaining <= 0 {
            return Verdict::reject(
                R::MinimumFreeSpace,
                "Importing after download will exceed available disk space",
            );
        }
        if remaining < minimum {
            return Verdict::reject(
                R::MinimumFreeSpace,
                format!(
                    "Not enough free space ({}) to import after download: {}. (Settings: Media Management: Minimum Free Space)",
                    size_suffix(minimum),
                    size_suffix(remaining)
                ),
            );
        }
        Verdict::Accept
    }
}
