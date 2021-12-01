use posenet_vr_hub::grpc::proto::enum_test::Myenum;
use posenet_vr_hub::grpc::proto::T1;

#[test]
fn test_enum() {
    let x = posenet_vr_hub::grpc::proto::EnumTest {
        myenum: Some(Myenum::T1(T1 {
            name: "Joe".to_owned(),
            age: 12,
        })),
    };
    if let Some(y) = x.myenum {
        match y {
            Myenum::T1(x) => {
                println!("T1: name = {}, age = {}", x.name, x.age)
            }
            Myenum::T2(x) => {
                println!("T2: color = {}, home = {}", x.color, x.home)
            }
        }
    }
}
