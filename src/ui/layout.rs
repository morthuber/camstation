use thiserror::Error;
use uuid::Uuid;

use crate::config::{MAX_GRID_EXTENT, ViewConfig, ViewTile};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(super) enum LayoutError {
    #[error("camera is not part of this view")]
    MissingCamera,
    #[error("tile overlaps another camera")]
    Overlap,
    #[error("grid dimensions must be between 1 and {MAX_GRID_EXTENT}")]
    InvalidDimensions,
    #[error("tile would exceed the maximum grid size")]
    GridLimit,
    #[error("grid cannot shrink while it contains tiles outside the new bounds")]
    OccupiedBounds,
    #[error("camera is already part of this view")]
    DuplicateCamera,
    #[error("view already contains ten cameras")]
    CameraLimit,
}

pub(super) fn move_tile(
    view: &ViewConfig,
    camera_id: Uuid,
    column: u32,
    row: u32,
) -> Result<ViewConfig, LayoutError> {
    update_tile(view, camera_id, |tile| {
        tile.column = column;
        tile.row = row;
    })
}

pub(super) fn resize_tile(
    view: &ViewConfig,
    camera_id: Uuid,
    column_span: u32,
    row_span: u32,
) -> Result<ViewConfig, LayoutError> {
    if column_span == 0 || row_span == 0 {
        return Err(LayoutError::InvalidDimensions);
    }
    update_tile(view, camera_id, |tile| {
        tile.column_span = column_span;
        tile.row_span = row_span;
    })
}

pub(super) fn set_dimensions(
    view: &ViewConfig,
    columns: u32,
    rows: u32,
) -> Result<ViewConfig, LayoutError> {
    if columns == 0 || rows == 0 || columns > MAX_GRID_EXTENT || rows > MAX_GRID_EXTENT {
        return Err(LayoutError::InvalidDimensions);
    }
    if view
        .tiles
        .iter()
        .any(|tile| tile.column + tile.column_span > columns || tile.row + tile.row_span > rows)
    {
        return Err(LayoutError::OccupiedBounds);
    }
    let mut candidate = view.clone();
    candidate.columns = columns;
    candidate.rows = rows;
    Ok(candidate)
}

pub(super) fn add_camera(view: &ViewConfig, camera_id: Uuid) -> Result<ViewConfig, LayoutError> {
    if view.tiles.iter().any(|tile| tile.camera_id == camera_id) {
        return Err(LayoutError::DuplicateCamera);
    }
    if view.tiles.len() >= 10 {
        return Err(LayoutError::CameraLimit);
    }

    let mut candidate = view.clone();
    let placement = (0..candidate.rows)
        .flat_map(|row| (0..candidate.columns).map(move |column| (column, row)))
        .find(|(column, row)| {
            !candidate
                .tiles
                .iter()
                .any(|tile| tile_contains(tile, *column, *row))
        });
    let (column, row) = match placement {
        Some(cell) => cell,
        None if candidate.rows < MAX_GRID_EXTENT => {
            let row = candidate.rows;
            candidate.rows += 1;
            (0, row)
        }
        None if candidate.columns < MAX_GRID_EXTENT => {
            let column = candidate.columns;
            candidate.columns += 1;
            (column, 0)
        }
        None => return Err(LayoutError::GridLimit),
    };
    candidate.tiles.push(ViewTile {
        camera_id,
        column,
        row,
        column_span: 1,
        row_span: 1,
    });
    Ok(candidate)
}

pub(super) fn remove_camera(view: &ViewConfig, camera_id: Uuid) -> Result<ViewConfig, LayoutError> {
    let mut candidate = view.clone();
    let original_len = candidate.tiles.len();
    candidate.tiles.retain(|tile| tile.camera_id != camera_id);
    if candidate.tiles.len() == original_len {
        return Err(LayoutError::MissingCamera);
    }
    Ok(candidate)
}

pub(super) fn tile(view: &ViewConfig, camera_id: Uuid) -> Option<&ViewTile> {
    view.tiles.iter().find(|tile| tile.camera_id == camera_id)
}

fn update_tile(
    view: &ViewConfig,
    camera_id: Uuid,
    update: impl FnOnce(&mut ViewTile),
) -> Result<ViewConfig, LayoutError> {
    let mut candidate = view.clone();
    let tile = candidate
        .tiles
        .iter_mut()
        .find(|tile| tile.camera_id == camera_id)
        .ok_or(LayoutError::MissingCamera)?;
    update(tile);
    let column_end = tile
        .column
        .checked_add(tile.column_span)
        .ok_or(LayoutError::GridLimit)?;
    let row_end = tile
        .row
        .checked_add(tile.row_span)
        .ok_or(LayoutError::GridLimit)?;
    if column_end > MAX_GRID_EXTENT || row_end > MAX_GRID_EXTENT {
        return Err(LayoutError::GridLimit);
    }
    candidate.columns = candidate.columns.max(column_end);
    candidate.rows = candidate.rows.max(row_end);
    if candidate.tiles.iter().enumerate().any(|(index, tile)| {
        candidate.tiles[index + 1..]
            .iter()
            .any(|other| tiles_overlap(tile, other))
    }) {
        return Err(LayoutError::Overlap);
    }
    Ok(candidate)
}

fn tile_contains(tile: &ViewTile, column: u32, row: u32) -> bool {
    column >= tile.column
        && column < tile.column + tile.column_span
        && row >= tile.row
        && row < tile.row + tile.row_span
}

fn tiles_overlap(left: &ViewTile, right: &ViewTile) -> bool {
    left.column < right.column + right.column_span
        && right.column < left.column + left.column_span
        && left.row < right.row + right.row_span
        && right.row < left.row + left.row_span
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> (ViewConfig, Uuid, Uuid) {
        let large = Uuid::new_v4();
        let small = Uuid::new_v4();
        (
            ViewConfig {
                id: Uuid::new_v4(),
                name: "Mixed".to_owned(),
                columns: 3,
                rows: 2,
                tiles: vec![
                    ViewTile {
                        camera_id: large,
                        column: 0,
                        row: 0,
                        column_span: 2,
                        row_span: 2,
                    },
                    ViewTile {
                        camera_id: small,
                        column: 2,
                        row: 0,
                        column_span: 1,
                        row_span: 1,
                    },
                ],
            },
            large,
            small,
        )
    }

    #[test]
    fn preserves_independent_mixed_spans() {
        let (view, large, small) = view();
        let moved = move_tile(&view, small, 2, 1).unwrap();
        assert_eq!(tile(&moved, large).unwrap().column_span, 2);
        assert_eq!(tile(&moved, small).unwrap().column_span, 1);
    }

    #[test]
    fn rejects_overlap_and_accepts_valid_resize() {
        let (view, _, small) = view();
        assert_eq!(move_tile(&view, small, 1, 0), Err(LayoutError::Overlap));
        let resized = resize_tile(&view, small, 1, 2).unwrap();
        assert_eq!(tile(&resized, small).unwrap().row_span, 2);
    }

    #[test]
    fn grows_at_edges_and_rejects_occupied_shrink() {
        let (view, _, small) = view();
        let moved = move_tile(&view, small, 3, 2).unwrap();
        assert_eq!((moved.columns, moved.rows), (4, 3));
        assert_eq!(
            set_dimensions(&view, 2, 2),
            Err(LayoutError::OccupiedBounds)
        );
    }

    #[test]
    fn camera_assignment_uses_free_cells_and_can_be_removed() {
        let (view, _, _) = view();
        let added_id = Uuid::new_v4();
        let added = add_camera(&view, added_id).unwrap();
        assert_eq!(
            (
                tile(&added, added_id).unwrap().column,
                tile(&added, added_id).unwrap().row
            ),
            (2, 1)
        );
        assert_eq!(remove_camera(&added, added_id).unwrap(), view);
    }

    #[test]
    fn enforces_grid_extent_limit() {
        let (view, _, small) = view();
        assert_eq!(
            move_tile(&view, small, MAX_GRID_EXTENT, 0),
            Err(LayoutError::GridLimit)
        );
        assert_eq!(
            set_dimensions(&view, MAX_GRID_EXTENT + 1, 2),
            Err(LayoutError::InvalidDimensions)
        );
    }
}
