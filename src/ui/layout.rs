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
    add_camera_at(&candidate, camera_id, column, row)
}

pub(super) fn add_camera_at(
    view: &ViewConfig,
    camera_id: Uuid,
    column: u32,
    row: u32,
) -> Result<ViewConfig, LayoutError> {
    if view.tiles.iter().any(|tile| tile.camera_id == camera_id) {
        return Err(LayoutError::DuplicateCamera);
    }
    if view.tiles.len() >= 10 {
        return Err(LayoutError::CameraLimit);
    }
    if column >= MAX_GRID_EXTENT || row >= MAX_GRID_EXTENT {
        return Err(LayoutError::GridLimit);
    }
    let placement = ViewTile {
        camera_id,
        column,
        row,
        column_span: 1,
        row_span: 1,
    };
    if !placement_is_available(view, &placement, None) {
        return Err(LayoutError::Overlap);
    }
    let mut candidate = view.clone();
    candidate.columns = candidate.columns.max(column + 1);
    candidate.rows = candidate.rows.max(row + 1);
    candidate.tiles.push(placement);
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

pub(super) fn fit_dimensions(view: &ViewConfig) -> ViewConfig {
    let mut fitted = view.clone();
    fitted.columns = fitted
        .tiles
        .iter()
        .map(|tile| tile.column + tile.column_span)
        .max()
        .unwrap_or(1)
        .max(1);
    fitted.rows = fitted
        .tiles
        .iter()
        .map(|tile| tile.row + tile.row_span)
        .max()
        .unwrap_or(1)
        .max(1);
    fitted
}

pub(super) fn placement_is_available(
    view: &ViewConfig,
    placement: &ViewTile,
    ignored_camera: Option<Uuid>,
) -> bool {
    placement.column_span > 0
        && placement.row_span > 0
        && placement
            .column
            .checked_add(placement.column_span)
            .is_some_and(|end| end <= MAX_GRID_EXTENT)
        && placement
            .row
            .checked_add(placement.row_span)
            .is_some_and(|end| end <= MAX_GRID_EXTENT)
        && !view
            .tiles
            .iter()
            .any(|tile| Some(tile.camera_id) != ignored_camera && tiles_overlap(tile, placement))
}

pub(super) fn cell_at(
    width: f64,
    height: f64,
    columns: u32,
    rows: u32,
    spacing: f64,
    x: f64,
    y: f64,
) -> Option<(u32, u32)> {
    if columns == 0 || rows == 0 || x < 0.0 || y < 0.0 || x >= width || y >= height {
        return None;
    }
    let cell_width =
        ((width - f64::from(columns.saturating_sub(1)) * spacing) / f64::from(columns)).max(1.0);
    let cell_height =
        ((height - f64::from(rows.saturating_sub(1)) * spacing) / f64::from(rows)).max(1.0);
    let column = (x / (cell_width + spacing)).floor() as u32;
    let row = (y / (cell_height + spacing)).floor() as u32;
    (column < columns && row < rows).then_some((column, row))
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
    use proptest::prelude::*;

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

    #[test]
    fn explicit_placement_and_fit_preserve_mixed_geometry() {
        let (view, _, _) = view();
        let added_id = Uuid::new_v4();
        assert_eq!(
            add_camera_at(&view, added_id, 0, 0),
            Err(LayoutError::Overlap)
        );
        let added = add_camera_at(&view, added_id, 4, 3).unwrap();
        assert_eq!((added.columns, added.rows), (5, 4));
        let fitted = fit_dimensions(&added);
        assert_eq!((fitted.columns, fitted.rows), (5, 4));
        let removed = remove_camera(&fitted, added_id).unwrap();
        assert_eq!(
            (
                fit_dimensions(&removed).columns,
                fit_dimensions(&removed).rows
            ),
            (3, 2)
        );
    }

    #[test]
    fn empty_view_fits_to_one_cell_and_internal_gaps_remain() {
        let empty = ViewConfig {
            id: Uuid::new_v4(),
            name: "Empty".to_owned(),
            columns: 8,
            rows: 8,
            tiles: Vec::new(),
        };
        assert_eq!(
            (fit_dimensions(&empty).columns, fit_dimensions(&empty).rows),
            (1, 1)
        );
        let (base, large, _) = view();
        let gapped = move_tile(&base, large, 2, 2).unwrap();
        let fitted = fit_dimensions(&gapped);
        assert_eq!((fitted.columns, fitted.rows), (4, 4));
        assert_eq!(
            (
                tile(&fitted, large).unwrap().column,
                tile(&fitted, large).unwrap().row
            ),
            (2, 2)
        );
    }

    #[test]
    fn pointer_coordinates_map_to_cells_and_ignore_spacing() {
        assert_eq!(cell_at(304.0, 204.0, 3, 2, 4.0, 10.0, 10.0), Some((0, 0)));
        assert_eq!(cell_at(304.0, 204.0, 3, 2, 4.0, 110.0, 10.0), Some((1, 0)));
        assert_eq!(cell_at(304.0, 204.0, 3, 2, 4.0, 303.0, 203.0), Some((2, 1)));
        assert_eq!(cell_at(304.0, 204.0, 3, 2, 4.0, -1.0, 0.0), None);
    }

    #[test]
    fn reports_all_assignment_errors_without_mutating_input() {
        let (view, large, _) = view();
        let original = view.clone();
        assert_eq!(add_camera(&view, large), Err(LayoutError::DuplicateCamera));
        assert_eq!(
            remove_camera(&view, Uuid::new_v4()),
            Err(LayoutError::MissingCamera)
        );
        assert_eq!(
            move_tile(&view, Uuid::new_v4(), 0, 0),
            Err(LayoutError::MissingCamera)
        );
        assert_eq!(
            resize_tile(&view, large, 0, 1),
            Err(LayoutError::InvalidDimensions)
        );
        assert_eq!(view, original);

        let mut full = view;
        while full.tiles.len() < 10 {
            full = add_camera(&full, Uuid::new_v4()).unwrap();
        }
        assert_eq!(
            add_camera(&full, Uuid::new_v4()),
            Err(LayoutError::CameraLimit)
        );
    }

    proptest! {
        #[test]
        fn moving_single_tile_preserves_layout_invariants(
            column in 0u32..MAX_GRID_EXTENT,
            row in 0u32..MAX_GRID_EXTENT,
            column_span in 1u32..=4,
            row_span in 1u32..=4,
        ) {
            let camera_id = Uuid::from_u128(1);
            let view = ViewConfig {
                id: Uuid::from_u128(2),
                name: "Property".to_owned(),
                columns: 1,
                rows: 1,
                tiles: vec![ViewTile {
                    camera_id,
                    column: 0,
                    row: 0,
                    column_span: 1,
                    row_span: 1,
                }],
            };
            let resized = resize_tile(&view, camera_id, column_span, row_span).unwrap();
            let result = move_tile(&resized, camera_id, column, row);
            if column + column_span <= MAX_GRID_EXTENT && row + row_span <= MAX_GRID_EXTENT {
                let moved = result.unwrap();
                let tile = tile(&moved, camera_id).unwrap();
                prop_assert_eq!((tile.column, tile.row), (column, row));
                prop_assert!(tile.column + tile.column_span <= moved.columns);
                prop_assert!(tile.row + tile.row_span <= moved.rows);
            } else {
                prop_assert_eq!(result, Err(LayoutError::GridLimit));
            }
        }

        #[test]
        fn adding_then_removing_camera_is_identity(seed in any::<u128>()) {
            let (view, _, _) = view();
            let mut id = Uuid::from_u128(seed);
            while view.tiles.iter().any(|tile| tile.camera_id == id) {
                id = Uuid::from_u128(id.as_u128().wrapping_add(1));
            }
            let added = add_camera(&view, id).unwrap();
            prop_assert_eq!(remove_camera(&added, id).unwrap(), view);
        }
    }
}
