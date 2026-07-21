use rusqlite::Connection;

use crate::error::{Error, Result};

// ─── Project CRUD ─────────────────────────────────────────────────────────────

use crate::models::project::Project;

pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, path, content_dir, site_url, site_id, sitemap_url, project_mode, active, agent_provider, seo_provider, clarity_project_id FROM projects ORDER BY name ASC",
    )?;
    let projects: Vec<Project> = stmt
        .query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                content_dir: row.get(3)?,
                site_url: row.get(4)?,
                site_id: row.get(5)?,
                sitemap_url: row.get(6)?,
                project_mode: row.get(7)?,
                active: row.get::<_, i64>(8)? != 0,
                agent_provider: row.get(9)?,
                seo_provider: row.get(10)?,
                clarity_project_id: row.get(11)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(projects)
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Project> {
    conn.query_row(
        "SELECT id, name, path, content_dir, site_url, site_id, sitemap_url, project_mode, active, agent_provider, seo_provider, clarity_project_id FROM projects WHERE id = ?1",
        [id],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                content_dir: row.get(3)?,
                site_url: row.get(4)?,
                site_id: row.get(5)?,
                sitemap_url: row.get(6)?,
                project_mode: row.get(7)?,
                active: row.get::<_, i64>(8)? != 0,
                agent_provider: row.get(9)?,
                seo_provider: row.get(10)?,
                clarity_project_id: row.get(11)?,
            })
        },
    )
    .map_err(|_| Error::Other(format!("Project '{id}' not found")))
}

pub fn create_project(conn: &Connection, project: &Project) -> Result<Project> {
    conn.execute(
        "INSERT INTO projects (id, name, path, content_dir, site_url, site_id, sitemap_url, project_mode, active, agent_provider, seo_provider, clarity_project_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            project.id,
            project.name,
            project.path,
            project.content_dir,
            project.site_url,
            project.site_id,
            project.sitemap_url,
            project.project_mode,
            project.active as i64,
            project.agent_provider,
            project.seo_provider,
            project.clarity_project_id,
        ],
    )?;
    get_project(conn, &project.id)
}

pub fn update_project(conn: &Connection, project: &Project) -> Result<Project> {
    let rows = conn.execute(
        "UPDATE projects SET name = ?1, path = ?2, content_dir = ?3, site_url = ?4, site_id = ?5, sitemap_url = ?6, project_mode = ?7, active = ?8, agent_provider = ?9, seo_provider = ?10, clarity_project_id = ?11
         WHERE id = ?12",
        rusqlite::params![
            project.name,
            project.path,
            project.content_dir,
            project.site_url,
            project.site_id,
            project.sitemap_url,
            project.project_mode,
            project.active as i64,
            project.agent_provider,
            project.seo_provider,
            project.clarity_project_id,
            project.id,
        ],
    )?;
    if rows == 0 {
        return Err(Error::Other(format!("Project '{}' not found", project.id)));
    }
    get_project(conn, &project.id)
}

pub fn delete_project(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    Ok(())
}
