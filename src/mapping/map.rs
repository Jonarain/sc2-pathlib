use crate::path_find::pos::Pos;
use crate::path_find::pos::MULTF64;
use crate::path_find::PathFind;
use pyo3::prelude::*;
use std::cmp;
extern crate test;
use std::collections::HashSet;

use crate::mapping::climb::modify_climb;
use crate::mapping::map_point;
use crate::mapping::map_point::Cliff;

const DIFFERENCE: usize = 16;
const Y_MULT: usize = 1000000;

/// Mapping for python-sc2
#[pyclass]
pub struct Map {
    pub ground_pathing: PathFind,
    pub air_pathing: PathFind,
    pub colossus_pathing: PathFind,
    pub reaper_pathing: PathFind,
    pub points: Vec<Vec<map_point::MapPoint>>,
    pub overlord_spots: Vec<(f64, f64)>,
    #[pyo3(get, set)]
    pub influence_colossus_map: bool,
    #[pyo3(get, set)]
    pub influence_reaper_map: bool,
    pub chokes: Vec::<((usize, usize), (usize, usize))>
}

#[pymethods]
impl Map {
    #[new]
    fn new_py(pathing: Vec<Vec<usize>>,
              placement: Vec<Vec<usize>>,
              height_map: Vec<Vec<usize>>,
              x_start: usize,
              y_start: usize,
              x_end: usize,
              y_end: usize)
              -> Self {
        Map::new(pathing, placement, height_map, x_start, y_start, x_end, y_end)
    }

    #[getter(ground_pathing)]
    fn get_ground_pathing(&self) -> PyResult<Vec<Vec<usize>>> { Ok(self.ground_pathing.map.clone()) }

    #[getter(overlord_spots)]
    fn get_overlord_spots(&self) -> Vec<(f64, f64)> { self.overlord_spots.clone() }

    #[getter(chokes)]
    fn get_chokes(&self) ->  Vec::<((usize, usize), (usize, usize))> { self.chokes.clone() }

    fn draw_climbs(&self) -> Vec<Vec<usize>> {
        let width = self.ground_pathing.map.len();
        let height = self.ground_pathing.map[0].len();
        let mut walk_map = vec![vec![0; height]; width];
        let path = &self.ground_pathing.map;

        for x in 0..width {
            for y in 0..height {
                if path[x][y] > 0 {
                    if self.points[x][y].cliff_type == Cliff::High {
                        walk_map[x][y] = 5;
                    } else if self.points[x][y].cliff_type == Cliff::Both {
                        walk_map[x][y] = 4;
                    } else if self.points[x][y].cliff_type == Cliff::Low {
                        walk_map[x][y] = 3;
                    } else {
                        walk_map[x][y] = 2;
                    }
                } else if self.points[x][y].climbable {
                    walk_map[x][y] = 1;
                } else if self.points[x][y].overlord_spot {
                    walk_map[x][y] = 6;
                }
            }
        }

        walk_map
    }

    fn draw_chokes(&self) -> Vec<Vec<usize>> {
        let width = self.ground_pathing.map.len();
        let height = self.ground_pathing.map[0].len();
        let mut walk_map = vec![vec![0; height]; width];

        for x in 0..width {
            for y in 0..height {
                let point = &self.points[x][y];
                if point.is_border {
                    if point.is_choke {
                        walk_map[x][y] = 175;
                    } else {
                        walk_map[x][y] = 255;
                    }
                }
                else if point.is_choke {
                    walk_map[x][y] = 100;
                }
            }
        }

        walk_map
    }

    /// Reset all mapping to their originals.
    fn reset(&mut self) {
        self.ground_pathing.reset_void();
        self.air_pathing.reset_void();
        self.colossus_pathing.reset_void();
        self.reaper_pathing.reset_void();
    }

    pub fn create_block(&mut self, center: (f32, f32), size: (usize, usize)) {
        self.ground_pathing.create_block(center, size);
        self.colossus_pathing.create_block(center, size);
        self.reaper_pathing.create_block(center, size);
    }

    pub fn create_blocks(&mut self, centers: Vec<(f32, f32)>, size: (usize, usize)) {
        self.ground_pathing.create_blocks_rust(&centers, size);
        self.colossus_pathing.create_blocks_rust(&centers, size);
        self.reaper_pathing.create_blocks_rust(&centers, size);
    }

    pub fn remove_blocks(&mut self, centers: Vec<(f32, f32)>, size: (usize, usize)) {
        self.ground_pathing.remove_blocks_rust(&centers, size);
        self.colossus_pathing.remove_blocks_rust(&centers, size);
        self.reaper_pathing.remove_blocks_rust(&centers, size);
    }

    pub fn add_influence_walk(&mut self, positions: Vec<(usize, usize)>, max: f64, distance: f64) -> PyResult<()> {
        let mult = 1.0 / distance;
        let max_int = max as usize;
        let mut maps = self.get_ground_influence_maps();

        for position in &positions {
            if maps[0].map[position.0][position.1] == 0 {
                continue;
            }

            let destinations = maps[0].find_destinations_in_inline(*position, distance);
            maps[0].map[position.0][position.1] += max_int;

            for destination in destinations {
                let end_point = destination.0;
                let current_distance = destination.1;
                let value = max * (1.0 - current_distance * mult);

                if current_distance < distance {
                    for mapping in maps.iter_mut() {
                        mapping.map[end_point.0][end_point.1] += value as usize
                    }
                }
            }
        }

        Ok(())
    }
}

impl Map {
    fn new(pathing: Vec<Vec<usize>>,
           placement: Vec<Vec<usize>>,
           height_map: Vec<Vec<usize>>,
           x_start: usize,
           y_start: usize,
           x_end: usize,
           y_end: usize)
           -> Self {
        let width = pathing.len();
        let height = pathing[0].len();
        let mut points = vec![vec![map_point::MapPoint::new(); height]; width];

        let mut walk_map = vec![vec![0; height]; width];
        let mut fly_map = vec![vec![0; height]; width];
        let mut reaper_map = vec![vec![0; height]; width];
        let mut overlord_spots: Vec<(f64, f64)> = Vec::new();

        let choke_distance = 13f64;
        let choke_border_distance = 30f64;

        let mut chokes = Vec::<((usize, usize), (usize, usize))>::new();

        // Pass 1
        for x in 0..width {
            for y in 0..height {
                let walkable = pathing[x][y] > 0 || placement[x][y] > 0;
                let pathable = x_start <= x && x <= x_end && y_start <= y && y <= y_end;
                points[x][y].walkable = walkable;
                points[x][y].pathable = pathable;
                points[x][y].height = height_map[x][y];

                if pathable {
                    fly_map[x][y] = 1;
                }
                if walkable {
                    walk_map[x][y] = 1;
                    reaper_map[x][y] = 1;
                }
            }
        }

        // Pass 2
        for x in x_start..x_end {
            for y in y_start..y_end {
                if !points[x][y].walkable {
                    let h0 = points[x][y + 1].height;
                    let h1 = points[x][y - 1].height;
                    if (points[x][y].height >= h0 + DIFFERENCE && h0 > 0)
                       || (points[x][y].height >= h1 + DIFFERENCE && h1 > 0)
                    {
                        points[x][y].overlord_spot = true;
                    }

                    if points[x + 1][y + 1].walkable
                       || points[x - 1][y + 1].walkable
                       || points[x + 1][y].walkable
                       || points[x - 1][y].walkable
                       || points[x + 1][y - 1].walkable
                       || points[x - 1][y - 1].walkable
                       || points[x][y + 1].walkable
                       || points[x][y - 1].walkable
                    {
                        points[x][y].is_border = true;
                    }

                    continue;
                }

                modify_climb(&mut points, x as i32, y as i32, -1, -1);
                modify_climb(&mut points, x as i32, y as i32, 1, -1);
                modify_climb(&mut points, x as i32, y as i32, 1, 0);
                modify_climb(&mut points, x as i32, y as i32, 0, 1);
            }
        }

        // Required for pass 3 choke detection
        let ground_pathing = PathFind::new_internal(walk_map);

        // Pass 3
        let mut set_handled_overlord_spots: HashSet<usize> = HashSet::new();
        for x in x_start..x_end {
            for y in y_start..y_end {
                let point_hash = x + y * Y_MULT;
                if points[x][y].climbable {
                    points[x][y].climbable = points[x + 1][y].climbable
                                             || points[x - 1][y].climbable
                                             || points[x][y + 1].climbable
                                             || points[x][y - 1].climbable;
                    if points[x][y].climbable {
                        reaper_map[x][y] = 1;
                    }
                }

                let pos_start = Pos(x, y);
                if points[pos_start.0][pos_start.1].is_border {
                    let reachable_borders = ground_pathing.invert_djiktra((x as f64, y as f64), choke_border_distance);
                    let xmin = cmp::max(x as i64 - choke_distance as i64, x_start as i64) as usize;
                    let xmax = cmp::min(x as i64 + choke_distance as i64, x_end as i64) as usize;
                    let ymin = cmp::max(y as i64 - choke_distance as i64, y_start as i64) as usize;
                    let ymax = cmp::min(y as i64 + choke_distance as i64, y_end as i64) as usize;

                    for x_new in xmin..xmax {
                        for y_new in ymin..ymax {
                            if !points[x_new][y_new].is_border {
                                // Needs to be a border to be acceptable position
                                continue;
                            }

                            let pos = Pos(x_new, y_new);
                            let flight_distance = pos.euclidean_distance(&pos_start) as f64 / MULTF64;

                            if flight_distance > choke_distance || flight_distance < 2f64 {
                                continue;
                            }

                            let mut found = false;
                            for pos_distance in &reachable_borders {
                                if (pos_distance.0).0 == x_new && (pos_distance.0).1 == y_new {
                                    found = true;
                                    break;
                                }
                            }

                            if found {
                                continue;
                            }

                            let dots = flight_distance as usize;
                            let unit_vector = ((pos.0 as f64 - x as f64) / flight_distance,
                                               (pos.1 as f64 - y as f64) / flight_distance);
                            let mut wall_hit = false;
                            let mut set_chokes = Vec::<(usize, usize)>::new();

                            for i in 1..dots {
                                let draw_x = (x as f64 + unit_vector.0 * i as f64) as usize;
                                let draw_y = (y as f64 + unit_vector.1 * i as f64) as usize;
                                if (draw_x == x && draw_y == y) || (draw_x == pos.0 && draw_y == pos.1) {
                                    continue;
                                }
                                if points[draw_x][draw_y].is_border {
                                    wall_hit = true;
                                    break;
                                } else {
                                    set_chokes.push((draw_x, draw_y));
                                    
                                }
                            }

                            if !wall_hit {
                                for dot in set_chokes {
                                    points[dot.0][dot.1].is_choke = true;
                                }
                                points[x][y].is_choke = true;
                                points[pos.0][pos.1].is_choke = true;
                                chokes.push(((x,y), (pos.0, pos.1)));
                            }
                        }
                    }
                }

                let c = points[x][y].cliff_type;

                if c != Cliff::None {
                    if points[x + 1][y].cliff_type != c
                       && points[x - 1][y].cliff_type != c
                       && points[x][y + 1].cliff_type != c
                       && points[x][y - 1].cliff_type != c
                    {
                        points[x][y].cliff_type = Cliff::None;
                    }
                }

                if !set_handled_overlord_spots.contains(&point_hash) && points[x][y].overlord_spot {
                    let target_height = points[x][y].height;
                    let mut set: HashSet<usize> = HashSet::new();

                    if flood_fill_overlord(&mut points, x, y, target_height, true, &mut set) {
                        let mut spot = (0_f64, 0_f64);
                        let count = set.len();
                        for value in set {
                            set_handled_overlord_spots.insert(value);
                            let cx = (value % Y_MULT) as f64;
                            let cy = (value / Y_MULT) as f64;
                            spot = (spot.0 + cx, spot.1 + cy);
                        }

                        spot = (spot.0 / count as f64, spot.1 / count as f64);
                        overlord_spots.push(spot);
                    } else {
                        set.clear();
                        flood_fill_overlord(&mut points, x, y, target_height, false, &mut set);
                    }
                }
            }
        }

        let air_pathing = PathFind::new_internal(fly_map);
        let colossus_pathing = PathFind::new_internal(reaper_map.clone());
        let reaper_pathing = PathFind::new_internal(reaper_map);

        let influence_colossus_map = false;
        let influence_reaper_map = false;

        Map { ground_pathing,
              air_pathing,
              colossus_pathing,
              reaper_pathing,
              points,
              overlord_spots,
              influence_colossus_map,
              influence_reaper_map,
              chokes }
    }

    fn get_ground_influence_maps(&mut self) -> Vec<&mut PathFind> {
        let mut maps = Vec::<&mut PathFind>::new();
        maps.push(&mut self.ground_pathing);

        if self.influence_colossus_map {
            maps.push(&mut self.colossus_pathing);
        }
        if self.influence_reaper_map {
            maps.push(&mut self.reaper_pathing);
        }

        return maps;
    }

    fn get_borders(&self) -> Vec<(usize, usize)> {
        let mut result = Vec::<(usize, usize)>::new();

        for x in 0..self.ground_pathing.width {
            for y in 0..self.ground_pathing.height {
                if self.points[x][y].is_border {
                    result.push((x, y));
                }
            }
        }

        return result;
    }
}

fn flood_fill_overlord(points: &mut Vec<Vec<map_point::MapPoint>>,
                       x: usize,
                       y: usize,
                       target_height: usize,
                       replacement: bool,
                       set: &mut HashSet<usize>)
                       -> bool {
    let key = x + y * Y_MULT;
    if set.contains(&key) {
        return true;
    }

    set.insert(key);

    if target_height != points[x][y].height {
        // Height difference must be at least 16 below target
        if target_height < points[x][y].height + DIFFERENCE {
            return false;
        }

        return true; // Could still be overlord spot.
    }

    let mut result = true;
    points[x][y].overlord_spot = replacement;
    // if points[x][y].overlord_spot == target {
    if y > 0 {
        result &= flood_fill_overlord(points, x, ((y as u32) - 1) as usize, target_height, replacement, set);
    }
    if x > 0 {
        result &= flood_fill_overlord(points, ((x as u32) - 1) as usize, y, target_height, replacement, set);
    }
    if y < points[0].len() - 1 {
        result &= flood_fill_overlord(points, x, y + 1, target_height, replacement, set);
    }
    if x < points.len() - 1 {
        result &= flood_fill_overlord(points, x + 1, y, target_height, replacement, set);
    }
    // }

    return result;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    fn read_vec_from_file(file_path: &str) -> Vec<Vec<usize>> {
        let f = BufReader::new(File::open(file_path).unwrap());
        let mut arr = Vec::<Vec<usize>>::new();

        for line in f.lines().map(|x| x.unwrap()) {
            let mut maze_line = vec![];
            for mini_line in line.chars().map(|n| n.to_digit(2).unwrap()) {
                maze_line.push(mini_line as usize)
            }

            arr.push(maze_line);
        }
        rotate90clockwise(arr)
    }

    fn rotate90clockwise(vec: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        let N = vec[0].len();
        let mut new_arr: Vec<Vec<usize>> = vec.clone();
        // Traverse each cycle
        for i in 0..(N / 2) {
            for j in i..(N - i - 1) {
                let temp = vec[i][j];
                new_arr[i][j] = vec[N - 1 - j][i];
                new_arr[N - 1 - j][i] = vec[N - 1 - i][N - 1 - j];
                new_arr[N - 1 - i][N - 1 - j] = vec[j][N - 1 - i];
                new_arr[j][N - 1 - j] = temp;
            }
        }
        new_arr
    }

    fn get_choke_map() -> Map {
        let grid = read_vec_from_file("tests/choke10x10.txt");
        let grid2 = read_vec_from_file("tests/choke10x10.txt");
        let grid3 = read_vec_from_file("tests/choke10x10.txt");

        let map = Map::new(grid, grid2, grid3, 2, 2, 12, 12);
        return map;
    }

    #[test]
    fn test_find_path_map() {
        let grid = read_vec_from_file("tests/maze4x4.txt");
        let grid2 = read_vec_from_file("tests/maze4x4.txt");
        let grid3 = read_vec_from_file("tests/maze4x4.txt");
        let map = Map::new(grid, grid2, grid3, 1, 1, 3, 3);
        let path_find = map.ground_pathing;
        let r = path_find.find_path((0, 0), (3, 3), Some(0));
        let (_, distance) = r.unwrap();
        assert_eq!(distance, 6.0);
    }

    // Test not working, ignored for now.
    // #[test]
    fn test_find_map_borders() {
        let map = get_choke_map();
        let r = map.get_borders();
        assert_eq!(r.len(), 20 + 16);
    }
}
