use alloy_primitives::{Address, address};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DVariant {
    Default,
    Group0,
    Group1,
    Group2,
    Group3,
    Group4,
    Legacy,
}

static D_VARIANT_GROUP_0: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("06364f10B501e868329afBc005b3492902d6C763"),
        address!("4CA9b3063Ec5866A4B82E437059D2C43d1be596F"),
        address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C"),
        address!("7fC77b5c7614E1533320Ea6DDc2Eb61fa00A9714"),
        address!("93054188d876f558f4a66B2EF1d97d16eDf0895B"),
        address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7"),
    ]
    .into_iter()
    .collect()
});

static D_VARIANT_GROUP_1: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("45F783CCE6B7FF23B2ab2D70e416cdb7D6055f51"),
        address!("79a8C46DeA5aDa233ABaFFD40F3A0A2B1e5A4F27"),
        address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56"),
        address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD"),
    ]
    .into_iter()
    .collect()
});

static D_VARIANT_GROUP_2: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("0AD66FeC8dB84F8A3365ADA04aB23ce607ac6E24"),
        address!("1c899dED01954d0959E034b62a728e7fEbE593b0"),
        address!("3F1B0278A9ee595635B61817630cC19DE792f506"),
        address!("3Fb78e61784C9c637D560eDE23Ad57CA1294c14a"),
        address!("453D92C7d4263201C69aACfaf589Ed14202d83a4"),
        address!("663aC72a1c3E1C4186CD3dCb184f216291F4878C"),
        address!("6A274dE3e2462c7614702474D64d376729831dCa"),
        address!("7C0d189E1FecB124487226dCbA3748bD758F98E4"),
        address!("875DF0bA24ccD867f8217593ee27253280772A97"),
        address!("99f5aCc8EC2Da2BC0771c32814EFF52b712de1E5"),
        address!("9D0464996170c6B9e75eED71c68B99dDEDf279e8"),
        address!("B37D6c07482Bc11cd28a1f11f1a6ad7b66Dec933"),
        address!("B657B895B265C38c53FFF00166cF7F6A3C70587d"),
        address!("D6Ac1CB9019137a896343Da59dDE6d097F710538"),
        address!("E95E4c2dAC312F31Dc605533D5A4d0aF42579308"),
        address!("f7b55C3732aD8b2c2dA7c24f30A69f55c54FB717"),
    ]
    .into_iter()
    .collect()
});

static D_VARIANT_GROUP_3: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("DC24316b9AE028F1497c275EB9192a3Ea0f67022"),
        address!("DeBF20617708857ebe4F679508E7b7863a8A8EeE"),
        address!("EB16Ae0052ed37f479f7fe63849198Df1765a733"),
    ]
    .into_iter()
    .collect()
});

static D_VARIANT_GROUP_4: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("1062FD8eD633c1f080754c19317cb3912810B5e5"),
        address!("1C5F80b6B68A9E1Ef25926EeE00b5255791b996B"),
        address!("26f3f26F46cBeE59d1F8860865e13Aa39e36A8c0"),
        address!("2d600BbBcC3F1B6Cb9910A70BaB59eC9d5F81B9A"),
        address!("320B564Fb9CF36933eC507a846ce230008631fd3"),
        address!("3b21C2868B6028CfB38Ff86127eF22E68d16d53B"),
        address!("69ACcb968B19a53790f43e57558F5E443A91aF22"),
        address!("971add32Ea87f10bD192671630be3BE8A11b8623"),
        address!("CA0253A98D16e9C1e3614caFDA19318EE69772D0"),
        address!("fBB481A443382416357fA81F16dB5A725DC6ceC8"),
    ]
    .into_iter()
    .collect()
});

pub fn get_d_variant(pool_address: &Address) -> DVariant {
    if D_VARIANT_GROUP_0.contains(pool_address) {
        DVariant::Group0
    } else if D_VARIANT_GROUP_1.contains(pool_address) {
        DVariant::Group1
    } else if D_VARIANT_GROUP_2.contains(pool_address) {
        DVariant::Group2
    } else if D_VARIANT_GROUP_3.contains(pool_address) {
        DVariant::Group3
    } else if D_VARIANT_GROUP_4.contains(pool_address) {
        DVariant::Group4
    } else {
        DVariant::Default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum YVariant {
    Default,
    Group0,
    Group1,
}

pub static Y_VARIANT_GROUP_0: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("45F783CCE6B7FF23B2ab2D70e416cdb7D6055f51"),
        address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C"),
        address!("79a8C46DeA5aDa233ABaFFD40F3A0A2B1e5A4F27"),
        address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56"),
        address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD"),
    ]
    .into_iter()
    .collect()
});

pub static Y_VARIANT_GROUP_1: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("06364f10B501e868329afBc005b3492902d6C763"),
        address!("45F783CCE6B7FF23B2ab2D70e416cdb7D6055f51"),
        address!("4CA9b3063Ec5866A4B82E437059D2C43d1be596F"),
        address!("52EA46506B9CC5Ef470C5bf89f17Dc28bB35D85C"),
        address!("79a8C46DeA5aDa233ABaFFD40F3A0A2B1e5A4F27"),
        address!("7fC77b5c7614E1533320Ea6DDc2Eb61fa00A9714"),
        address!("93054188d876f558f4a66B2EF1d97d16eDf0895B"),
        address!("A2B47E3D5c44877cca798226B7B8118F9BFb7A56"),
        address!("A5407eAE9Ba41422680e2e00537571bcC53efBfD"),
        address!("bEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7"),
    ]
    .into_iter()
    .collect()
});

pub fn get_y_variant(pool_address: &Address) -> YVariant {
    if Y_VARIANT_GROUP_0.contains(pool_address) {
        YVariant::Group0
    } else if Y_VARIANT_GROUP_1.contains(pool_address) {
        YVariant::Group1
    } else {
        YVariant::Default
    }
}

pub static Y_D_VARIANT_GROUP_0: Lazy<HashSet<Address>> = Lazy::new(|| {
    [
        address!("DcEF968d416a41Cdac0ED8702fAC8128A64241A2"),
        address!("f253f83AcA21aAbD2A20553AE0BF7F65C755A07F"),
    ]
    .into_iter()
    .collect()
});
