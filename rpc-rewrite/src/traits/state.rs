pub trait State {
    type ClientHandles: super::Method;

    type ServerHandles: super::Method;
}
