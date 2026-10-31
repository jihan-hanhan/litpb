import nbtlib
from nbtlib.tag import IntArray , Int , String , Compound , List
from PIL import Image
import sys

y = 0

result = Compound({})
values = {
    "path":"",
    "scale":1.0,
    "output":"",
    "grid":16,
    "block":"littletiles:ltcoloredblock",
    "resampling":0
}

def _help():
    print('''
        Version: 0.0.1
        Commands:
          -i [path_to_image] : Image Input @MUST
          -o [path_to_file]  : Export result as .snbt file
          -s [Scale]         : Scale
            -s-re [Int var] : Resampling Type
          -g [grid]          : Notice : Must be a multiple of 4(max=32,min=1)
          -b [blockID]       : Select block type (nor:littletiles:ltcoloredblock)
          #--un_splash       : #Without output logs
        PIL Resampling enum:
          NEAREST = 0   BOX = 4
          BILINEAR = 2  HAMMING = 5
          BICUBIC = 3   LANCZOS = 1
          MKS2013 = 6   MKS2021 = 7
        ''')

def img_to_snbt(path:str,scale:float=1.0,grid:int=0,block:str="littletiles:ltcoloredblock",resampling:int=0):
    #图片处理
    img = Image.open(path).convert("RGBA") #读图
    width , height = img.size
    img = img.resize((int(width*scale),int(height*scale)),resample=Image.Resampling(resampling)) #缩放

    #值
    width , height = img.size
    tiles = List[Compound]([])

    pixels = img.load() 
    count = 0
    bar_count = 0
    bar_cells = width*height
    #警告
    if bar_cells >= 10000:
        print("\033[31mWarning : The Result was TOO BIG to load with LittleTiles . Are you SURE to make that?\033[0m")
    #转化为蓝图
    for x in range(width):
        for z in range(height):
            r,g,b,a = pixels[x,z]

            #跳过空白部分
            if a == 0:
                bar_count += 1 #弥补空白部分
                process_bar(bar_count,bar_cells)
                continue
            
            #转化颜色
            r &= 0xFF
            g &= 0xFF
            b &= 0xFF
            a &= 0xFF
            color = (a << 24) | (r << 16) | (g << 8) | b #换算为int

            #限位
            if color >= 0x80000000:
                color -= 0x100000000
            
            #组装
            cell = Compound({
                "bBox":IntArray([x,y,z,x+1,y+1,z+1]),
                "tile":Compound({
                    "color": Int(color),
                    "block": String(block)
                })
            })
            tiles.append(cell)
            count += 1
            bar_count += 1
            process_bar(bar_count,bar_cells)
    print()
    
    #最终组装
    _result = nbtlib.Compound({
        "tiles":tiles,
        "min":IntArray([0,0,0]),
        "size":IntArray([width,y+1,height]),
        "grid":Int(grid),
        "count":Int(count)
    })
    return _result
    
def flow():
    print("\033[32mStart chores\033[0m")

    #获取结果
    print(f"With Image \033[33m{values['path']}\033[0m")
    print(f"With Scale \033[33m{values['scale']}\033[0m")
    print(f"With grid \033[33m{values['scale']}\033[0m")
    print(f"With block \033[33m{values['block']}\033[0m")
    print(f"With resampling \033[33m{str(Image.Resampling(values['resampling']))}\033[0m")
    #计算
    result = img_to_snbt(values["path"],values["scale"],values["grid"],values['block'],values["resampling"])

    #保存文件
    if not values["output"] == "":
        with open(values["output"],"w",encoding="utf-8") as file:
            file.write(nbtlib.serialize_tag(result))
            print(f"Saved file at \033[33m{values['output']}\033[0m")

    print("\033[32mAll chores done!\033[0m")

    print("\033[0m")

#一个简单的procesbar
def process_bar(var:int,end:int):
    print("\r",end="")
    bar = "━" * int(20 * var/end) + "─" * (20 - int(20 * var/end))
    print(f"Process[\033[36m{bar}\033[0m] {var}/{end}",end="")
    sys.stdout.flush()

def _cmd():
    argv = sys.argv

    if len(argv) == 1:
        _help()
        return

    it = iter(argv[1:])

    for cmd in it:
        if cmd == "help":
            _help()
            return

        match cmd:
            case "-i":
                image_path = next(it, None)

                if image_path is None:
                    print("Fail : Path to Image is None")
                    return
                
                values["path"]=image_path
            
            case "-o":
                output_path = next(it,None)
                
                if output_path is None:
                    print("Fail : Invalid output path")
                    return
                
                values["output"]=str(output_path)
            
            case "-s":
                scale = next(it,None)

                if scale is None:
                    print("Fali : Invalid Scale")
                    return
                
                values["scale"]=float(scale)
            
            case "-g":
                grid = next(it,None)
                
                if grid is None or (int(grid)%4!=0 and int(grid)!=1) or int(grid)>32 or int(grid) <= 0:
                    print("Fail : Invalid grid")
                    return
                
                values["grid"]=int(grid)
            
            case "-b":
                block = next(it,None)
                
                if block is None:
                    print("Fail : Invalid Block")
                    return
                
                values['block']=block
            
            case "-s-re":
                re = next(it,None)

                if re is None or int(re) < 0 or int(re) > 8:
                    print("Fail : Invaild Resampling type")

            case _:
                print(f"未知参数：{cmd}")
                _help()
                return

def _mian():
    _cmd()
    if len(sys.argv) !=1:
        flow()

if __name__ == "__main__":
    _mian()