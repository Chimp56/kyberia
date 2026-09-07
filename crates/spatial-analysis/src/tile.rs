use crate::*;
use kyberia_domain::{
    identity::{FloorId, FrameId},
    units::Meters,
};
use serde::Serialize;

/// Row-major cell centers: origin + ((column + 0.5), (row + 0.5)) * resolution.
/// Integer offsets allow independently computed tiles to use identical centers.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Grid {
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub origin: Point2,
    pub resolution: Meters,
    pub column_offset: u32,
    pub row_offset: u32,
    pub width: u32,
    pub height: u32,
}
impl Grid {
    fn center(self, column: u32, row: u32) -> Result<Point2, Error> {
        let x = (f64::from(self.column_offset) + f64::from(column) + 0.5) * self.resolution.get()
            + self.origin.x.get();
        let y = (f64::from(self.row_offset) + f64::from(row) + 0.5) * self.resolution.get()
            + self.origin.y.get();
        Ok(Point2 {
            x: CoordinateMeters::new(x)
                .map_err(|_| Error::NumericalFailure("grid x coordinate"))?,
            y: CoordinateMeters::new(y)
                .map_err(|_| Error::NumericalFailure("grid y coordinate"))?,
        })
    }
    pub fn validate(self) -> Result<usize, Error> {
        let count = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or(Error::ResourceLimit("cell count"))?;
        if count == 0 || count > MAX_CELLS {
            return Err(Error::ResourceLimit("cell count"));
        }
        if self.resolution.get() <= 0.0 {
            return Err(Error::InvalidConfiguration(
                "positive grid resolution required",
            ));
        }
        if self.column_offset.checked_add(self.width).is_none()
            || self.row_offset.checked_add(self.height).is_none()
        {
            return Err(Error::ResourceLimit("grid offsets"));
        }
        self.center(0, 0)?;
        self.center(self.width - 1, self.height - 1)?;
        // A grid finer than the representable coordinate spacing is not a grid.
        for column in 1..self.width {
            if self.center(column - 1, 0)?.x >= self.center(column, 0)?.x {
                return Err(Error::NumericalFailure("unrepresentable grid x spacing"));
            }
        }
        for row in 1..self.height {
            if self.center(0, row - 1)?.y >= self.center(0, row)?.y {
                return Err(Error::NumericalFailure("unrepresentable grid y spacing"));
            }
        }
        Ok(count)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tile {
    pub schema_version: &'static str,
    pub algorithm_version: &'static str,
    pub coincident_aggregation: &'static str,
    pub inputs: Inputs,
    pub configuration: Config,
    pub location_groups: Vec<LocationGroup>,
    pub grid: Grid,
    pub cells: Vec<Cell>,
}
impl Model {
    /// All-or-error; cancellation never returns incomplete cells as complete data.
    pub fn tile(&self, grid: Grid, mut cancelled: impl FnMut() -> bool) -> Result<Tile, Error> {
        if grid.floor_id != self.inputs.floor_id {
            return Err(Error::FloorMismatch);
        }
        if grid.frame_id != self.inputs.frame_id {
            return Err(Error::FrameMismatch);
        }
        let count = grid.validate()?;
        if count
            .checked_mul(self.groups.len())
            .is_none_or(|work| work > MAX_DISTANCE_EVALUATIONS)
        {
            return Err(Error::ResourceLimit(
                "distance evaluations; request smaller tiles",
            ));
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut cells = Vec::with_capacity(count);
        for row in 0..grid.height {
            for column in 0..grid.width {
                cells.push(self.estimate(grid.center(column, row)?, &mut cancelled)?);
            }
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Tile {
            schema_version: "kyberia.numeric-rssi-tile/1",
            algorithm_version: ALGORITHM_VERSION,
            coincident_aggregation: "arithmetic-mean-dbm/1",
            inputs: self.inputs.clone(),
            configuration: self.config,
            location_groups: self.groups.clone(),
            grid,
            cells,
        })
    }
}
