use std::io::{Read, Seek, SeekFrom, Error, BufReader};
use std::collections::{HashMap};

use byteorder::{LittleEndian, ReadBytesExt};
use linked_hash_map::LinkedHashMap;

use armake::io::*;

pub struct Point {
    pub coords: (f32, f32, f32),
    pub flags: u32
}

pub struct Vertex {
    pub point_index: u32,
    pub normal_index: u32,
    pub uv: (f32, f32)
}

pub struct Face {
    pub vertices: Vec<Vertex>,
    pub flags: u32,
    pub texture: String,
    pub material: String
}

pub struct Selection {
    pub points: HashMap<Point, f32>,
    pub faces: HashMap<Face, f32>
}

pub struct LOD {
    pub version_major: u32,
    pub version_minor: u32,
    pub resolution: f32,
    pub points: Vec<Point>,
    pub face_normals: Vec<(f32, f32, f32)>,
    pub faces: Vec<Face>,
    pub sharp_edges: Vec<(u32, u32)>,
    pub selections: LinkedHashMap<String, Selection>,
    pub properties: LinkedHashMap<String, String>
}

pub struct P3D {
    pub version: u32,
    pub lods: Vec<LOD>
}

impl Point {
    fn read<I: Read>(input: &mut I) -> Result<Point, Error> {
        Ok(Point {
            coords: (input.read_f32::<LittleEndian>()?, input.read_f32::<LittleEndian>()?, input.read_f32::<LittleEndian>()?),
            flags: input.read_u32::<LittleEndian>()?
        })
    }
}

impl Vertex {
    fn read<I: Read>(input: &mut I) -> Result<Vertex, Error> {
        Ok(Vertex {
            point_index: input.read_u32::<LittleEndian>()?,
            normal_index: input.read_u32::<LittleEndian>()?,
            uv: (input.read_f32::<LittleEndian>()?, input.read_f32::<LittleEndian>()?)
        })
    }
}

impl Face {
    fn read<I: Read>(input: &mut I) -> Result<Face, Error> {
        let num_verts = input.read_u32::<LittleEndian>()?;
        assert!(num_verts == 3 || num_verts == 4);

        let mut verts: Vec<Vertex> = Vec::with_capacity(num_verts as usize);
        for _i in 0..num_verts {
            verts.push(Vertex::read(input)?);
        }

        if num_verts == 3 {
            Vertex::read(input)?;
        }

        let flags = input.read_u32::<LittleEndian>()?;
        let texture = input.read_cstring()?;
        let material = input.read_cstring()?;

        Ok(Face {
            vertices: verts,
            flags: flags,
            texture: texture,
            material: material
        })
    }
}

impl LOD {
    fn read<I: Read + Seek>(input: &mut I, deps_only: bool) -> Result<LOD, Error> {
        let mut buffer = [0; 4];
        input.read_exact(&mut buffer)?;
        assert_eq!(&buffer, b"P3DM");

        let version_major = input.read_u32::<LittleEndian>()?;
        let version_minor = input.read_u32::<LittleEndian>()?;

        let num_points = input.read_u32::<LittleEndian>()?;
        let num_face_normals = input.read_u32::<LittleEndian>()?;
        let num_faces = input.read_u32::<LittleEndian>()?;

        input.bytes().nth(3);

        let mut points: Vec<Point>;
        let mut face_normals: Vec<(f32, f32, f32)>;
        if deps_only {
            points = Vec::with_capacity(num_points as usize);
            for _i in 0..num_points {
                points.push(Point::read(input)?);
            }

            face_normals = Vec::with_capacity(num_face_normals as usize);
            for _i in 0..num_face_normals {
                face_normals.push((input.read_f32::<LittleEndian>()?, input.read_f32::<LittleEndian>()?, input.read_f32::<LittleEndian>()?));
            }
        } else {
            points = Vec::new();
            face_normals = Vec::new();
            input.seek(SeekFrom::Current((num_points * 16 + num_face_normals * 12) as i64))?;
        }

        let mut faces: Vec<Face> = Vec::with_capacity(num_faces as usize);
        for _i in 0..num_faces {
            faces.push(Face::read(input)?);
        }

        input.read_exact(&mut buffer)?;
        assert_eq!(&buffer, b"TAGG");

        loop {
            input.bytes().next();

            let name = input.read_cstring()?;
            let size = input.read_u32::<LittleEndian>()?;
            //let mut buffer = vec![0; size as usize].into_boxed_slice();
            //input.read_exact(&mut buffer)?;

            input.seek(SeekFrom::Current(size as i64))?;

            if name == "#EndOfFile#" { break; }
            // @todo: handle others
        }

        let resolution = input.read_f32::<LittleEndian>()?;

        Ok(LOD {
            version_major: version_major,
            version_minor: version_minor,
            resolution: resolution,
            points: points,
            face_normals: face_normals,
            faces: faces,
            sharp_edges: Vec::new(),
            selections: LinkedHashMap::new(),
            properties: LinkedHashMap::new()
        })
    }
}

impl P3D {
    pub fn read<I: Read + Seek>(input: &mut I, deps_only: bool) -> Result<P3D, Error> {
        let mut reader = BufReader::new(input);

        let mut buffer = [0; 4];
        reader.read_exact(&mut buffer)?;
        assert_eq!(&buffer, b"MLOD");

        let version = reader.read_u32::<LittleEndian>()?;
        let num_lods = reader.read_u32::<LittleEndian>()?;
        let mut lods: Vec<LOD> = Vec::with_capacity(num_lods as usize);

        for _i in 0..num_lods {
            lods.push(LOD::read(&mut reader, deps_only)?);
        }

        Ok(P3D {
            version: version,
            lods: lods
        })
    }
}
