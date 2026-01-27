use super::content::*;
use super::Object::*;
use super::{Dictionary, Document, Object, Stream};
use image::{self, ColorType, GenericImage};
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;

impl Document {
	pub fn insert_image<P: AsRef<Path>>(&mut self, page_number: u32, path: P, position: (i32, i32), scale: (f32, f32)) {
		let img = image::open(&path).unwrap();
		let (width, height) = img.dimensions();
		let (color_space, bits) = match img.color() {
			ColorType::Gray(bits) => (b"DeviceGray".to_vec(), bits),
			ColorType::RGB(bits) => (b"DeviceRGB".to_vec(), bits),
			ColorType::Palette(bits) => (b"DeviceN".to_vec(), bits),
			ColorType::GrayA(bits) => (b"DeviceN".to_vec(), bits),
			ColorType::RGBA(bits) => (b"DeviceN".to_vec(), bits),
		};

		let mut dict = Dictionary::new();
		dict.set("Type", Name(b"XObject".to_vec()));
		dict.set("SubType", Name(b"Image".to_vec()));
		dict.set("Width", width);
		dict.set("Height", height);
		dict.set("ColorSpace", Name(color_space));
		dict.set("BitsPerComponent", bits);
		// let mut img_object = Stream::new(dict, img.raw_pixels());
		// img_object.compress();

		// dict.set("Filter", Name(b"JPXDecode".to_vec()));
		dict.set("Filter", Name(b"FlateDecode".to_vec()));
		let mut f = File::open("/Users/Junfeng/Rust/lopdf/pdfutil/(2, 0).bin").unwrap();
		let mut buffer = Vec::new();

		// read the whole file
		f.read_to_end(&mut buffer).unwrap();
		let img_object = Stream::new(dict, buffer);

		let img_id = self.add_object(img_object);
		let img_name = "Img1";

		let pages = self.get_pages();
		let page_id = *pages.get(&page_number).expect(&format!("Page {} not exist.", page_number));
		let mut content = self.get_and_decode_page_content(page_id);
		content.operations.push(Operation::new("q", vec![]));
		content
			.operations
			.push(Operation::new("cm", vec![scale.0.into(), 0.into(), 0.into(), scale.1.into(), position.0.into(), position.1.into()]));
		content.operations.push(Operation::new("gs", vec![Name("G0".as_bytes().to_vec())]));
		content.operations.push(Operation::new("Do", vec![Name(img_name.as_bytes().to_vec())]));
		content.operations.push(Operation::new("Q", vec![]));
		let modified_contnet = content.encode().unwrap();

		let mut gs = Dictionary::new();
		gs.set("ca", Integer(1));
		gs.set("BM", Name(b"Normal".to_vec()));
		let gs_id = self.add_object(gs);

		self.add_proc(page_id, b"PDF".to_vec());
		self.add_proc(page_id, b"ImageB".to_vec());
		self.add_proc(page_id, b"ImageC".to_vec());
		self.add_graphics_state(page_id, "G0", gs_id);
		self.add_xobject(page_id, img_name, img_id);
		self.change_page_content(page_id, modified_contnet);
	}

	pub fn insert_watermark(&mut self, page_number: u32, position: (i32, i32)) {
		let mut dict = Dictionary::new();
		dict.set("Type", Name(b"XObject".to_vec()));
		dict.set("SubType", Name(b"Form".to_vec()));
		dict.set("BBox", Array(vec![0.into(), 0.into(), 100.into(), 100.into()]));
		dict.set("Matrix", Array(vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()]));
		let watermark = Stream::new(
			dict,
			"0 0 m
0 1000 l
1000 1000 l
1000 0 l
f
"
				.as_bytes()
				.to_vec(),
		);

		let form_name = "Fm1";
		let form_id = self.add_object(watermark);

		let pages = self.get_pages();
		let page_id = *pages.get(&page_number).expect(&format!("Page {} not exist.", page_number));
		let mut content = self.get_and_decode_page_content(page_id);
		// content.operations.push(Operation::new("q", vec![]));
		// content
		// 	.operations
		// 	.push(Operation::new("cm", vec![0.into(), 0.into(), 0.into(), 0.into(), position.0.into(), position.1.into()]));
		content.operations.push(Operation::new("Do", vec![Name(form_name.as_bytes().to_vec())]));
		// content.operations.push(Operation::new("Q", vec![]));
		let modified_contnet = content.encode().unwrap();

		self.add_xobject(page_id, form_name, form_id);
		self.change_page_content(page_id, modified_contnet);
	}
}
#[test]
fn insert_image() {
	let mut doc = Document::load("assets/example.pdf").unwrap();
	doc.insert_image(1, "/Users/Junfeng/ZhimoProjects/zhimo-tiku-job-queue/1.png", (100, 100), (1.0, 1.0));
	// doc.insert_image(1, "/Users/Junfeng/Downloads/newkv2.png", (100, 100), (1.0, 1.0));
	// doc.insert_watermark(1, (100, 100));
	doc.save("test_5_image.pdf").unwrap();
}
