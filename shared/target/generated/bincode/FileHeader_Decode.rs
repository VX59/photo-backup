impl < __Context > :: bincode :: Decode < __Context > for FileHeader
{
    fn decode < __D : :: bincode :: de :: Decoder < Context = __Context > >
    (decoder : & mut __D) ->core :: result :: Result < Self, :: bincode ::
    error :: DecodeError >
    {
        core :: result :: Result ::
        Ok(Self
        {
            file_name : :: bincode :: Decode :: decode(decoder) ?, file_size :
            :: bincode :: Decode :: decode(decoder) ?, file_ext : :: bincode
            :: Decode :: decode(decoder) ?,
        })
    }
} impl < '__de, __Context > :: bincode :: BorrowDecode < '__de, __Context >
for FileHeader
{
    fn borrow_decode < __D : :: bincode :: de :: BorrowDecoder < '__de,
    Context = __Context > > (decoder : & mut __D) ->core :: result :: Result <
    Self, :: bincode :: error :: DecodeError >
    {
        core :: result :: Result ::
        Ok(Self
        {
            file_name : :: bincode :: BorrowDecode ::< '_, __Context >::
            borrow_decode(decoder) ?, file_size : :: bincode :: BorrowDecode
            ::< '_, __Context >:: borrow_decode(decoder) ?, file_ext : ::
            bincode :: BorrowDecode ::< '_, __Context >::
            borrow_decode(decoder) ?,
        })
    }
}