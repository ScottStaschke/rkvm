use crate::abs::{AbsAxis, AbsInfo};
use crate::event::Event;
use crate::key::Key;
use crate::rel::RelAxis;

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::io::Error;

#[cfg(target_os = "linux")]
pub use {crate::linux::writer::WriterLinux as Writer, crate::linux::writer::WriterLinuxBuilder as WriterBuilder};

#[async_trait]
pub trait EventWriter {
    async fn event(&mut self, id: usize, event: Event) -> Result<(), Error> ;
}
#[async_trait]
pub trait DeviceWriter: EventWriter {
    async fn create_device(&mut self, id: usize, name: &CString, vendor: u16, product: u16, version: u16, rel: HashSet<RelAxis>, abs: HashMap<AbsAxis, AbsInfo>, keys: HashSet<Key>, delay: Option<i32>, period: Option<i32>) -> Result<(), Error>;
    async fn destroy_device(&mut self, id: usize) -> Result<(), Error>;
}

#[async_trait]
impl<T> EventWriter for Box<T>
where
    T: DeviceWriter + ?Sized + Send,
{
    async fn event(&mut self, id: usize, event: Event) -> Result<(), Error> {
        (**self).event(id, event).await
    }
}
#[async_trait]
impl<T> DeviceWriter for Box<T>
where
    T: DeviceWriter + ?Sized + Send,
{
    async fn create_device(&mut self, id: usize, name: &CString, vendor: u16, product: u16, version: u16, rel: HashSet<RelAxis>, abs: HashMap<AbsAxis, AbsInfo>, keys: HashSet<Key>, delay: Option<i32>, period: Option<i32>) -> Result<(), Error> {
        (**self).create_device(id, name, vendor, product, version, rel, abs, keys, delay, period).await
    }
    async fn destroy_device(&mut self, id: usize) -> Result<(), Error> {
        (**self).destroy_device(id).await
    }
}

pub trait WriterPlatform {
    type Builder: WriterBuilderPlatform;

    fn builder() -> Result<Self::Builder, Error>;

    fn write<'a>(&'a mut self, event: &'a Event) -> impl std::future::Future<Output = Result<(), Error>> + Send + 'a;
}

pub trait WriterBuilderPlatform: Sized {
    type Writer: WriterPlatform;

    fn name(self, name: &CString) -> Self;

    fn vendor(self, value: u16) -> Self;

    fn product(self, value: u16) -> Self;

    fn version(self, value: u16) -> Self;

    fn rel<T: IntoIterator<Item = RelAxis>>(self, items: T) -> Result<Self, Error>;

    fn abs<T: IntoIterator<Item = (AbsAxis, AbsInfo)>>(self, items: T) -> Result<Self, Error>;

    fn key<T: IntoIterator<Item = Key>>(self, items: T) -> Result<Self, Error>;

    fn delay(self, value: Option<i32>) -> Result<Self, Error>;

    fn period(self, value: Option<i32>) -> Result<Self, Error>;

    fn build(self) -> impl std::future::Future<Output = Result<Self::Writer, Error>> + Send;
}
