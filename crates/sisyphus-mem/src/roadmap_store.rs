use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::collections::HashMap;

use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase, Project};

/// Structured storage for the Sisyphus Roadmap.
/// Handles relational mapping of Project -> Milestone -> Phase -> Goal.
pub struct RoadmapSqlStore {
    pool: SqlitePool,
}

impl RoadmapSqlStore {
    /// Open a [`RoadmapSqlStore`] on the given `pool`.
    ///
    /// Schema must be initialized separately via [`Self::init_schema`];
    /// the constructor is intentionally cheap so it can be called from
    /// multiple tasks before any of them touch the schema.
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        Ok(Self { pool })
    }

    /// Initializes the relational schema for the roadmap.
    pub async fn init_schema(&self) -> Result<()> {
        let schema = r#"
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                trajectory TEXT
            );

            CREATE TABLE IF NOT EXISTS milestones (
                id TEXT,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                status TEXT NOT NULL,
                PRIMARY KEY (id, project_id),
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS phases (
                id TEXT,
                project_id TEXT NOT NULL,
                milestone_id TEXT NOT NULL,
                name TEXT NOT NULL,
                depends_on TEXT,
                PRIMARY KEY (id, project_id),
                FOREIGN KEY(milestone_id, project_id) REFERENCES milestones(id, project_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS goals (
                id TEXT,
                project_id TEXT NOT NULL,
                phase_id TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL,
                PRIMARY KEY (id, project_id),
                FOREIGN KEY(phase_id, project_id) REFERENCES phases(id, project_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_outcomes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                goal_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                milestone_id TEXT NOT NULL,
                phase_id TEXT NOT NULL,
                success BOOLEAN NOT NULL,
                cost REAL NOT NULL,
                duration REAL NOT NULL,
                timestamp INTEGER NOT NULL,
                FOREIGN KEY(goal_id, project_id) REFERENCES goals(id, project_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_outcomes_goal ON execution_outcomes(goal_id);
            CREATE INDEX IF NOT EXISTS idx_milestones_project ON milestones(project_id);
            CREATE INDEX IF NOT EXISTS idx_phases_milestone ON phases(project_id, milestone_id);
            CREATE INDEX IF NOT EXISTS idx_goals_phase ON goals(project_id, phase_id);
        "#;

        sqlx::query(schema)
            .execute(&self.pool)
            .await
            .context("Failed to initialize roadmap schema")?;
        Ok(())
    }

    // ── Project Operations ────────────────────────────────────────────────

    /// Insert or replace a [`Project`] record keyed by `project.id`.
    pub async fn insert_project(&self, project: &Project) -> Result<()> {
        sqlx::query("INSERT INTO projects (id, name, description) VALUES (?, ?, ?)")
            .bind(&project.id)
            .bind(&project.name)
            .bind(&project.description)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// List all project ids known to the roadmap, sorted alphabetically.
    pub async fn list_projects(&self) -> Result<Vec<String>> {
        let rows =
            sqlx::query_as::<_, (String,)>("SELECT id FROM projects").fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Delete a project and cascade-delete its milestones, phases, and goals.
    ///
    /// No-op if the project does not exist.
    pub async fn delete_project(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Load a project by id, returning `None` if it does not exist.
    pub async fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let project_row = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT id, name, description FROM projects WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((pid, name, description)) = project_row else {
            return Ok(None);
        };

        let milestones_rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, name, status FROM milestones WHERE project_id = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        // Three fixed queries regardless of project size. The previous form
        // issued one query per milestone and then one per phase, so loading a
        // project with M milestones and P phases each cost 1 + M + M*P round
        // trips.
        let phases_rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "SELECT id, name, milestone_id, depends_on FROM phases WHERE project_id = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let goals_rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, description, status, phase_id FROM goals WHERE project_id = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let decode_status = |raw: &str| match raw {
            "Completed" => MilestoneStatus::Completed,
            "Blocked" => MilestoneStatus::Blocked,
            "InProgress" => MilestoneStatus::InProgress,
            _ => MilestoneStatus::Pending,
        };

        let mut goals_by_phase: HashMap<String, Vec<GoalRef>> = HashMap::new();
        for (g_id, g_desc, g_status, phase_id) in goals_rows {
            goals_by_phase.entry(phase_id).or_default().push(GoalRef {
                goal_id: g_id,
                description: g_desc,
                status: decode_status(&g_status),
            });
        }

        let mut phases_by_milestone: HashMap<String, Vec<Phase>> = HashMap::new();
        for (p_id, p_name, milestone_id, p_deps) in phases_rows {
            let depends_on = p_deps
                .map(|s| s.split(',').filter(|x| !x.is_empty()).map(String::from).collect())
                .unwrap_or_default();
            let goals = goals_by_phase.remove(&p_id).unwrap_or_default();
            phases_by_milestone.entry(milestone_id).or_default().push(Phase {
                id: p_id,
                name: p_name,
                goals,
                depends_on,
            });
        }

        let milestones = milestones_rows
            .into_iter()
            .map(|(m_id, m_name, m_status_str)| Milestone {
                id: m_id.clone(),
                name: m_name,
                status: decode_status(&m_status_str),
                phases: phases_by_milestone.remove(&m_id).unwrap_or_default(),
            })
            .collect();

        Ok(Some(Project {
            id: pid,
            name,
            description: description.unwrap_or_default(),
            roadmap: sisyphus_types::Roadmap { milestones },
        }))
    }

    // ── Milestone Operations ──────────────────────────────────────────────

    /// Insert or replace a [`Milestone`] under a project.
    pub async fn insert_milestone(
        &self,
        id: &str,
        project_id: &str,
        name: &str,
        status: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO milestones (id, project_id, name, status) VALUES (?, ?, ?, ?) 
                     ON CONFLICT(id, project_id) DO UPDATE SET name=excluded.name, status=excluded.status")
            .bind(id)
            .bind(project_id)
            .bind(name)
            .bind(status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update the [`MilestoneStatus`] of a single milestone, identified by
    /// `(project_id, milestone_id)`.
    pub async fn update_milestone_status(
        &self,
        project_id: &str,
        id: &str,
        status: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE milestones SET status = ? WHERE id = ? AND project_id = ?")
            .bind(status)
            .bind(id)
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a milestone and cascade-delete its phases and goals.
    pub async fn delete_milestone(&self, project_id: &str, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM milestones WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Phase Operations ──────────────────────────────────────────────────

    /// Insert or replace a [`Phase`] under a milestone.
    pub async fn insert_phase(
        &self,
        project_id: &str,
        id: &str,
        milestone_id: &str,
        name: &str,
        depends_on: Option<String>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO phases (id, project_id, milestone_id, name, depends_on) VALUES (?, ?, ?, ?, ?) 
                     ON CONFLICT(id, project_id) DO UPDATE SET name=excluded.name, depends_on=excluded.depends_on")
            .bind(id)
            .bind(project_id)
            .bind(milestone_id)
            .bind(name)
            .bind(depends_on)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Goal Operations ───────────────────────────────────────────────────

    /// Insert or replace a [`GoalRef`] under a phase.
    pub async fn insert_goal(
        &self,
        project_id: &str,
        id: &str,
        phase_id: &str,
        description: &str,
        status: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO goals (id, project_id, phase_id, description, status) VALUES (?, ?, ?, ?, ?) 
                     ON CONFLICT(id, project_id) DO UPDATE SET description=excluded.description, status=excluded.status")
            .bind(id)
            .bind(project_id)
            .bind(phase_id)
            .bind(description)
            .bind(status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Insert a milestone with all its phases and goals in one transaction.
    ///
    /// The manager's per-row inserts committed individually, so a failure part
    /// way through left a milestone with only some of its phases, and a large
    /// roadmap cost one round trip per row.
    pub async fn insert_milestone_tree(
        &self,
        project_id: &str,
        milestone: &sisyphus_types::roadmap::Milestone,
    ) -> Result<()> {
        use sqlx::Acquire;
        let mut conn = self.pool.acquire().await?;

        let mut tx = conn.begin().await?;

        sqlx::query(
            "INSERT INTO milestones (id, project_id, name, status) VALUES (?, ?, ?, ?)
             ON CONFLICT(id, project_id) DO UPDATE SET name=excluded.name, status=excluded.status",
        )
        .bind(&milestone.id)
        .bind(project_id)
        .bind(&milestone.name)
        .bind(milestone.status.to_string())
        .execute(&mut *tx)
        .await?;

        for phase in &milestone.phases {
            let deps = phase.depends_on.join(",");
            sqlx::query(
                "INSERT INTO phases (id, project_id, milestone_id, name, depends_on)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(id, project_id) DO UPDATE SET name=excluded.name, depends_on=excluded.depends_on",
            )
            .bind(&phase.id)
            .bind(project_id)
            .bind(&milestone.id)
            .bind(&phase.name)
            .bind(&deps)
            .execute(&mut *tx)
            .await?;

            for goal in &phase.goals {
                sqlx::query(
                    "INSERT INTO goals (id, project_id, phase_id, description, status)
                     VALUES (?, ?, ?, ?, ?)
                     ON CONFLICT(id, project_id) DO UPDATE SET description=excluded.description, status=excluded.status",
                )
                .bind(&goal.goal_id)
                .bind(project_id)
                .bind(&phase.id)
                .bind(&goal.description)
                .bind(goal.status.to_string())
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    /// Update the textual status of a single goal.
    pub async fn update_goal_status(&self, project_id: &str, id: &str, status: &str) -> Result<()> {
        sqlx::query("UPDATE goals SET status = ? WHERE id = ? AND project_id = ?")
            .bind(status)
            .bind(id)
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a single goal, leaving its parent phase and milestone intact.
    pub async fn delete_goal(&self, project_id: &str, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM goals WHERE id = ? AND project_id = ?")
            .bind(id)
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Fetch a goal's description field by id.
    ///
    /// Returns `None` if the goal does not exist; an empty string in the
    /// description column is returned as `Some("")`.
    pub async fn get_goal_description(
        &self,
        project_id: &str,
        goal_id: &str,
    ) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT description FROM goals WHERE id = ? AND project_id = ?",
        )
        .bind(goal_id)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|r| r.0))
    }

    // ── Analytics Queries (The "Reason" for structured storage) ────────────────

    /// Directly find goals that are bottlenecks (high failure rate) using SQL.
    pub async fn get_bottleneck_goals(&self, min_failures: i64) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT goal_id, COUNT(*) as failure_count 
             FROM execution_outcomes 
             WHERE success = 0 
             GROUP BY goal_id 
             HAVING failure_count >= ?",
        )
        .bind(min_failures)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Calculate overall project progress via SQL aggregation.
    pub async fn calculate_project_progress(&self, project_id: &str) -> Result<f64> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT 
                COUNT(DISTINCT g.id) as total, 
                COUNT(DISTINCT CASE WHEN g.status = 'Completed' THEN g.id END) as completed 
             FROM goals g 
             JOIN phases p ON g.phase_id = p.id AND g.project_id = p.project_id
             JOIN milestones m ON p.milestone_id = m.id AND p.project_id = m.project_id
             WHERE m.project_id = ?",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((total, completed)) = row {
            if total == 0 {
                return Ok(0.0);
            }
            Ok(completed as f64 / total as f64)
        } else {
            Ok(0.0)
        }
    }

    /// Update only the trajectory field of a project, leaving everything else intact.
    pub async fn update_project_trajectory(&self, id: &str, trajectory: &str) -> Result<()> {
        sqlx::query("UPDATE projects SET trajectory = ? WHERE id = ?")
            .bind(trajectory)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
